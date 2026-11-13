//! Where search results come from. Each source is a provider: apps, settings, files and the web search, and people
//! and mail when a Microsoft 365 account is signed in and they're switched on. Quick providers answer within the same
//! frame as the key press; the others work in the background and say when they have something. The menu lists their
//! rows in the order the person chose, the best match first.

use crate::apps::App;
use crate::config::{Category, Config};
use crate::i18n::tr;
use crate::index::{Facets, FileFilter, Hit, Index};
use crate::m365::graph::{self, Message, Person};
use crate::m365::{self, Feature, Session};
use crate::policy::Policies;
use crate::search::{self, FileSearch, Icon, Row, RowKind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// What every provider is told about a search.
pub struct Ask<'a> {
    pub query: &'a str,
    pub tokens: &'a [String],
    pub cfg: &'a Config,
    pub policy: &'a Policies,
    /// The file type buttons switched on, with regular expressions on or off.
    pub filter: &'a FileFilter,
    /// The Microsoft 365 account, when one is signed in.
    pub m365: Option<&'a Arc<Session>>,
}

/// One source of search results.
pub trait Provider {
    fn category(&self) -> Category;
    /// A new search, or the same one again because something changed. Quick providers answer here; the others start.
    fn ask(&mut self, ask: &Ask);
    /// The search box is empty again.
    fn clear(&mut self);
    /// Takes in an answer that came in the background. True when the rows changed.
    fn poll(&mut self) -> bool {
        false
    }
    /// The rows of the latest answer, best first.
    fn rows(&self, cfg: &Config) -> Vec<Row>;
    /// The search is plainly about this kind of result, so it goes ahead of the order the person chose.
    fn leads(&self) -> bool {
        false
    }
}

pub struct Apps {
    list: Arc<RwLock<Vec<App>>>,
    rows: Vec<Row>,
}

impl Provider for Apps {
    fn category(&self) -> Category {
        Category::Apps
    }

    fn ask(&mut self, ask: &Ask) {
        let apps = self.list.read().unwrap();
        self.rows = search::app_rows(&apps, ask.cfg, ask.tokens);
    }

    fn clear(&mut self) {
        self.rows.clear();
    }

    fn rows(&self, _: &Config) -> Vec<Row> {
        self.rows.clone()
    }
}

pub struct Settings {
    rows: Vec<Row>,
    lead: bool,
    /// The words that name the setting, without "switch to" and the like.
    pub subject: Vec<String>,
}

impl Provider for Settings {
    fn category(&self) -> Category {
        Category::Settings
    }

    fn ask(&mut self, ask: &Ask) {
        let mut found = search::setting_rows(ask.tokens);
        let (changes, pages) = (ask.cfg.search_windows_settings, ask.cfg.search_settings_pages);
        found.rows.retain(|row| {
            ask.policy.allows_row(&row.kind)
                && match &row.kind {
                    RowKind::Quick(..) => changes,
                    RowKind::Run(crate::actions::Do::Open(_)) => pages,
                    _ => true,
                }
        });
        found.rows.truncate(ask.cfg.max_settings.max(1));
        self.rows = found.rows;
        self.lead = found.lead;
        self.subject = found.subject;
    }

    fn clear(&mut self) {
        self.rows.clear();
        self.lead = false;
        self.subject.clear();
    }

    fn rows(&self, _: &Config) -> Vec<Row> {
        self.rows.clone()
    }

    fn leads(&self) -> bool {
        self.lead
    }
}

pub struct Files {
    search: FileSearch,
    waiting: u64,
    hits: Vec<Hit>,
    pub facets: Facets,
    /// How long the last answer took, once there is one.
    pub micros: Option<u64>,
    /// Why a regular expression in the search couldn't be used.
    pub error: Option<String>,
}

impl Provider for Files {
    fn category(&self) -> Category {
        Category::Files
    }

    fn ask(&mut self, ask: &Ask) {
        self.waiting = self.search.request(ask.query, ask.filter, ask.cfg.max_files.max(1));
    }

    fn clear(&mut self) {
        self.facets = Facets::default();
        self.hits.clear();
        self.micros = None;
        self.waiting = self.search.request("", &FileFilter::default(), 0);
    }

    fn poll(&mut self) -> bool {
        match self.search.take_answer() {
            Some(answer) if answer.generation == self.waiting => {
                self.hits = answer.hits;
                self.facets = answer.facets;
                self.micros = Some(answer.micros);
                self.error = answer.error;
                true
            }
            _ => false,
        }
    }

    fn rows(&self, cfg: &Config) -> Vec<Row> {
        search::file_rows(&self.hits, cfg)
    }
}

pub struct Web {
    row: Option<Row>,
}

impl Provider for Web {
    fn category(&self) -> Category {
        Category::Web
    }

    fn ask(&mut self, ask: &Ask) {
        self.row = search::web_row(ask.query, ask.cfg);
    }

    fn clear(&mut self) {
        self.row = None;
    }

    fn rows(&self, _: &Config) -> Vec<Row> {
        self.row.iter().cloned().collect()
    }
}

/// A search Microsoft Graph answers: people or mail. It waits for a pause in the typing, needs two letters, and only
/// asks while its feature is switched on.
pub struct Graph365 {
    feature: Feature,
    tx: Sender<(u64, String, Arc<Session>)>,
    latest: Arc<AtomicU64>,
    done: Arc<Mutex<Option<(u64, Vec<Row>)>>>,
    rows: Vec<Row>,
}

/// How long typing has to pause before Graph is asked.
const PAUSE: Duration = Duration::from_millis(300);

impl Graph365 {
    fn start(feature: Feature, ctx: egui::Context) -> Graph365 {
        let (tx, rx) = mpsc::channel::<(u64, String, Arc<Session>)>();
        let latest = Arc::new(AtomicU64::new(0));
        let done: Arc<Mutex<Option<(u64, Vec<Row>)>>> = Arc::default();
        let (latest_worker, done_worker) = (latest.clone(), done.clone());
        let _ = std::thread::Builder::new().name("m365-search".into()).spawn(move || {
            crate::platform::com_init();
            while let Ok(mut request) = rx.recv() {
                loop {
                    match rx.recv_timeout(PAUSE) {
                        Ok(newer) => request = newer,
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                let (generation, query, session) = request;
                if latest_worker.load(Ordering::Relaxed) != generation {
                    continue;
                }
                // Anything that goes wrong just means no rows: the Microsoft 365 page says what's wrong with the account.
                let rows = match feature {
                    Feature::People => session.graph.get(&graph::people_path(&query), &graph::PEOPLE_HEADERS).map(|json| people_rows(graph::parse_people(&json))),
                    _ => session.graph.get(&graph::mail_path(&query), &[]).map(|json| mail_rows(graph::parse_mail(&json))),
                }
                .unwrap_or_default();
                if latest_worker.load(Ordering::Relaxed) == generation {
                    *done_worker.lock().unwrap() = Some((generation, rows));
                    ctx.request_repaint();
                }
            }
        });
        Graph365 { feature, tx, latest, done, rows: Vec::new() }
    }
}

impl Graph365 {
    /// For a picture: these rows, as if Graph had answered.
    pub fn show(&mut self, rows: Vec<Row>) {
        self.rows = rows;
    }
}

impl Provider for Graph365 {
    fn category(&self) -> Category {
        if self.feature == Feature::People { Category::People } else { Category::Mail }
    }

    fn ask(&mut self, ask: &Ask) {
        let generation = self.latest.fetch_add(1, Ordering::Relaxed) + 1;
        let wanted = m365::feature_on(ask.cfg, self.feature) && ask.cfg.shows(self.category()) && ask.query.trim().chars().count() >= 2;
        match ask.m365.filter(|_| wanted) {
            Some(session) => {
                let _ = self.tx.send((generation, ask.query.trim().to_string(), session.clone()));
            }
            None => self.rows.clear(),
        }
    }

    fn clear(&mut self) {
        self.latest.fetch_add(1, Ordering::Relaxed);
        self.rows.clear();
    }

    fn poll(&mut self) -> bool {
        let answer = self.done.lock().unwrap().take();
        match answer {
            Some((generation, rows)) if generation == self.latest.load(Ordering::Relaxed) => {
                self.rows = rows;
                true
            }
            _ => false,
        }
    }

    fn rows(&self, _: &Config) -> Vec<Row> {
        self.rows.clone()
    }
}

/// A person in the results: their name, and their job and mail beneath. Enter opens a chat with them.
pub fn people_rows(people: Vec<Person>) -> Vec<Row> {
    people
        .into_iter()
        .map(|person| {
            let detail = [person.title.as_str(), person.mail.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(", ");
            Row { category: Category::People, name: person.name.clone(), detail, icon: Icon::Glyph('\u{E77B}'), kind: RowKind::Person(person), path: None }
        })
        .collect()
}

/// A message in the results: its subject, and who sent it and when beneath. Enter opens it in Outlook on the web.
pub fn mail_rows(messages: Vec<Message>) -> Vec<Row> {
    let today = crate::m365::calendar::Day::today();
    messages
        .into_iter()
        .map(|message| {
            let local = crate::platform::local_time(message.received);
            let when = if (local.year, local.month, local.day) == (today.year, today.month, today.day) {
                crate::i18n::fmt_time(local.hour, local.minute)
            } else {
                crate::i18n::fmt_date(local.year, local.month, local.day)
            };
            let detail = [message.from.as_str(), when.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(", ");
            let name = if message.subject.is_empty() { tr("(no subject)").to_string() } else { message.subject.clone() };
            Row { category: Category::Mail, name, detail, icon: Icon::Glyph('\u{E715}'), kind: RowKind::Link(message.web_link), path: None }
        })
        .collect()
}

/// Every source of search results.
pub struct Providers {
    pub apps: Apps,
    pub settings: Settings,
    pub files: Files,
    pub web: Web,
    pub people: Graph365,
    pub mail: Graph365,
}

impl Providers {
    pub fn start(apps: Arc<RwLock<Vec<App>>>, index: Arc<RwLock<Index>>, ctx: egui::Context) -> Providers {
        Providers {
            apps: Apps { list: apps, rows: Vec::new() },
            settings: Settings { rows: Vec::new(), lead: false, subject: Vec::new() },
            files: Files { search: FileSearch::start(index, ctx.clone()), waiting: 0, hits: Vec::new(), facets: Facets::default(), micros: None, error: None },
            web: Web { row: None },
            people: Graph365::start(Feature::People, ctx.clone()),
            mail: Graph365::start(Feature::Mail, ctx),
        }
    }

    pub fn each(&self) -> [&dyn Provider; 6] {
        [&self.apps, &self.settings, &self.files, &self.web, &self.people, &self.mail]
    }

    pub fn each_mut(&mut self) -> [&mut dyn Provider; 6] {
        [&mut self.apps, &mut self.settings, &mut self.files, &mut self.web, &mut self.people, &mut self.mail]
    }

    pub fn of(&self, category: Category) -> &dyn Provider {
        self.each().into_iter().find(|p| p.category() == category).unwrap_or(&self.web)
    }
}
