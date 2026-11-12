//! Puts search together. Apps and settings are few, so they are answered within the same
//! frame as the key press. Files get a helper thread: a newer key press cancels older work,
//! and the menu keeps drawing no matter what.

use crate::actions::{ACTIONS, Do};
use crate::apps::App;
use crate::config::{Category, Config, Target};
use crate::index::{Facets, FileFilter, Hit, Index};
use crate::intent::{self, Want};
use crate::i18n::{tr, tx};
use crate::matching;
use crate::settings::QuickId;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, RwLock};

pub struct FileSearch {
    tx: Sender<(u64, String, FileFilter, usize)>,
    latest: Arc<AtomicU64>,
    done: Arc<Mutex<Option<FileAnswer>>>,
}

pub struct FileAnswer {
    pub generation: u64,
    pub hits: Vec<Hit>,
    pub facets: Facets,
    pub micros: u64,
    /// Why a regular expression in the search couldn't be used.
    pub error: Option<String>,
}

impl FileSearch {
    pub fn start(index: Arc<RwLock<Index>>, ctx: egui::Context) -> FileSearch {
        let (tx, rx) = mpsc::channel::<(u64, String, FileFilter, usize)>();
        let latest = Arc::new(AtomicU64::new(0));
        let done: Arc<Mutex<Option<FileAnswer>>> = Arc::default();
        let (latest_worker, done_worker) = (latest.clone(), done.clone());
        std::thread::Builder::new()
            .name("search".into())
            .spawn(move || {
                while let Ok(mut request) = rx.recv() {
                    for newer in rx.try_iter() {
                        request = newer;
                    }
                    let (generation, query, filter, limit) = request;
                    let started = std::time::Instant::now();
                    let answer = {
                        let Ok(index) = index.read() else { continue };
                        index.search_filtered(&query, &filter, limit, &|| latest_worker.load(Ordering::Relaxed) != generation)
                    };
                    if latest_worker.load(Ordering::Relaxed) == generation {
                        let micros = started.elapsed().as_micros() as u64;
                        *done_worker.lock().unwrap() = Some(FileAnswer { generation, hits: answer.hits, facets: answer.facets, micros, error: answer.error });
                        ctx.request_repaint();
                    }
                }
            })
            .expect("search thread");
        FileSearch { tx, latest, done }
    }

    pub fn request(&self, query: &str, filter: &FileFilter, limit: usize) -> u64 {
        let generation = self.latest.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.tx.send((generation, query.to_string(), filter.clone(), limit));
        generation
    }

    pub fn take_answer(&self) -> Option<FileAnswer> {
        self.done.lock().unwrap().take()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Icon {
    Image { key: String, source: String },
    Glyph(char),
}

impl Icon {
    pub fn for_target(target: &Target) -> Icon {
        match target {
            Target::App(id) => Icon::Image { key: format!("app:{id}"), source: format!("shell:AppsFolder\\{id}") },
            Target::Path { path, .. } if path.contains("://") => Icon::Glyph('\u{E774}'),
            Target::Path { path, .. } => {
                let is_dir = crate::platform::stat(path).is_some_and(|s| s.0);
                Icon::Image { key: crate::icons::file_icon_key(path, is_dir), source: path.clone() }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RowKind {
    Open(Target),
    Run(Do),
    Quick(QuickId, Want),
    /// A web search for these words.
    Web(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub category: Category,
    pub name: String,
    pub detail: String,
    pub icon: Icon,
    pub kind: RowKind,
    /// Where it lives on disk, for "show in folder" and "copy path".
    pub path: Option<String>,
}

/// Groups of file types offered as one filter button.
pub const FILE_KINDS: &[(&str, &[&str])] = &[
    (tx("Documents"), &[".pdf", ".doc", ".docx", ".txt", ".rtf", ".odt", ".xls", ".xlsx", ".csv", ".ppt", ".pptx", ".md"]),
    (tx("Pictures"), &[".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp", ".svg", ".heic", ".tif", ".tiff"]),
    (tx("Videos"), &[".mp4", ".mkv", ".mov", ".avi", ".wmv", ".webm"]),
    (tx("Music"), &[".mp3", ".wav", ".flac", ".m4a", ".ogg", ".aac"]),
    (tx("Archives"), &[".zip", ".rar", ".7z", ".tar", ".gz", ".iso"]),
];

const MAX_PAGES: usize = 4;

pub fn app_rows(apps: &[App], cfg: &Config, tokens: &[String]) -> Vec<Row> {
    let matches: Vec<(i32, &App)> = apps
        .iter()
        .filter_map(|app| {
            let score = matching::score_all(&app.lower, &app.alias, tokens)?;
            (score >= 120).then_some((score, app))
        })
        .collect();
    // Scattered letters only help when nothing really matches; otherwise they are noise.
    let any_real = matches.iter().any(|(score, _)| *score > matching::LOOSE);
    let mut scored: Vec<(i32, &App)> = matches
        .into_iter()
        .filter(|(score, _)| !any_real || *score > matching::LOOSE)
        .map(|(score, app)| (score + cfg.usage_boost(&Target::App(app.id.clone()).key()), app))
        .collect();
    scored.sort_by_key(|(score, app)| (std::cmp::Reverse(*score), app.lower.len()));
    scored
        .into_iter()
        .take(cfg.max_apps.max(1))
        .map(|(_, app)| {
            let target = Target::App(app.id.clone());
            Row {
                category: Category::Apps,
                name: app.name.clone(),
                detail: tr("App").into(),
                icon: Icon::for_target(&target),
                kind: RowKind::Open(target),
                path: (!app.path.is_empty()).then(|| app.path.clone()),
            }
        })
        .collect()
}

pub struct SettingRows {
    pub rows: Vec<Row>,
    /// The search is plainly about settings, so they go ahead of apps and files.
    pub lead: bool,
    /// The words that name the setting, without "switch to" and the like.
    pub subject: Vec<String>,
}

pub fn setting_rows(tokens: &[String]) -> SettingRows {
    let found = intent::find(tokens);
    // Live controls get a head start: finding "volume" should give you the slider, not a page.
    let mut scored: Vec<(i32, Row)> = found
        .hits
        .iter()
        .map(|hit| {
            let info = hit.id.info();
            (
                hit.score + 100,
                Row {
                    category: Category::Settings,
                    name: tr(info.label).into(),
                    detail: String::new(),
                    icon: Icon::Glyph(info.glyph),
                    kind: RowKind::Quick(hit.id, hit.want),
                    path: None,
                },
            )
        })
        .collect();
    let mut pages: Vec<(i32, Row)> = Vec::new();
    for action in ACTIONS {
        // Found by its English name and by the name shown, in German or Italian.
        let (name, shown) = (matching::lower(action.name), matching::lower(tr(action.name)));
        let best = |words: &[String]| [&name, &shown].into_iter().filter_map(|n| matching::score_all(n, action.keywords, words)).max();
        let score = best(tokens).or_else(|| best(&found.subject));
        if let Some(score) = score.filter(|s| *s >= 300) {
            pages.push((
                score,
                Row {
                    category: Category::Settings,
                    name: tr(action.name).into(),
                    detail: match action.what {
                        Do::Open(_) => tr("Windows settings").into(),
                        _ => tr("Action").into(),
                    },
                    icon: Icon::Glyph(action.glyph),
                    kind: RowKind::Run(action.what),
                    path: None,
                },
            ));
        }
    }
    pages.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    pages.truncate(MAX_PAGES);
    scored.extend(pages);
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    SettingRows { rows: scored.into_iter().map(|(_, row)| row).collect(), lead: found.lead, subject: found.subject }
}

pub fn file_rows(hits: &[Hit], cfg: &Config) -> Vec<Row> {
    let mut rows: Vec<(i32, Row)> = hits
        .iter()
        .map(|hit| {
            let target = Target::path(hit.path.clone());
            let boost = cfg.usage_boost(&target.key());
            let parent = hit.path.rsplit_once('\\').map(|(dir, _)| dir).unwrap_or("").to_string();
            (
                hit.score + boost,
                Row {
                    category: Category::Files,
                    name: hit.name.clone(),
                    detail: parent,
                    icon: Icon::Image {
                        key: crate::icons::file_icon_key(&hit.path, hit.is_dir),
                        source: hit.path.clone(),
                    },
                    kind: RowKind::Open(target),
                    path: Some(hit.path.clone()),
                },
            )
        })
        .collect();
    rows.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    rows.into_iter().map(|(_, row)| row).collect()
}

pub fn web_row(query: &str, cfg: &Config) -> Option<Row> {
    if !cfg.shows(Category::Web) || query.trim().is_empty() {
        return None;
    }
    // Startline's own default has a whole sentence of its own in each language; a name the person or an administrator
    // chose for a fixed address stays as they wrote it. The browser's own engine has no name here.
    let name = if !cfg.web_fixed || cfg.web_name == "the web" { // i18n-ignore: the saved default
        crate::trf!("Search the web for \"{query}\"", query = query.trim())
    } else {
        crate::trf!("Search {name} for \"{query}\"", name = cfg.web_name, query = query.trim())
    };
    Some(Row {
        category: Category::Web,
        name,
        detail: String::new(),
        icon: Icon::Glyph('\u{E774}'),
        kind: RowKind::Web(query.trim().to_string()),
        path: None,
    })
}

/// How well a name fits what was typed: 4 the whole name (a file's name without its type counts too), 3 the name
/// starts with it, 2 a word in the name starts with it, 1 it's somewhere in the name, 0 not at all.
pub fn name_fit(name: &str, typed: &str) -> u8 {
    let typed = matching::lower(typed.trim());
    if typed.is_empty() {
        return 0;
    }
    let name = matching::lower(name.trim());
    let stem = name.rsplit_once('.').map_or("", |(stem, _)| stem);
    if name == typed || stem == typed {
        4
    } else if name.starts_with(&typed) {
        3
    } else if name.match_indices(&typed).any(|(at, _)| name[..at].chars().next_back().is_some_and(|c| !c.is_alphanumeric())) {
        2
    } else if name.contains(&typed) {
        1
    } else {
        0
    }
}

/// Which of these rows is the best match for what was typed, by how well its name fits, or one of its `other_names`
/// (an app's program and package names, which find "Editor" by "notepad"). Ties go to the kind shown first, then to
/// what was opened most often (`uses`), then to the row that came first. Web searches and rows whose name doesn't hold
/// the text at all never are.
pub fn best_match(rows: &[Row], typed: &str, kind_order: &[Category], other_names: impl Fn(&Row) -> String, uses: impl Fn(&Row) -> u32) -> Option<usize> {
    let fit = |row: &Row| {
        let others = other_names(row);
        std::iter::once(name_fit(&row.name, typed)).chain(others.split_whitespace().map(|other| name_fit(other, typed))).max().unwrap_or(0)
    };
    rows.iter()
        .enumerate()
        .filter(|(_, row)| row.category != Category::Web)
        .map(|(at, row)| (at, fit(row), kind_order.iter().position(|k| *k == row.category).unwrap_or(usize::MAX), uses(row)))
        .filter(|(_, fit, _, _)| *fit > 0)
        .max_by(|a, b| a.1.cmp(&b.1).then(b.2.cmp(&a.2)).then(a.3.cmp(&b.3)).then(b.0.cmp(&a.0)))
        .map(|(at, ..)| at)
}

/// The search address with the words in it, for a fixed address or a browser that can't be asked.
pub fn web_address(cfg: &Config, query: &str) -> String {
    cfg.web_url.replace("{}", &percent_encode(query.trim()))
}

/// A typed path or web address opens directly.
pub fn direct_row(query: &str) -> Option<Row> {
    let text = query.trim().trim_matches('"');
    let looks_web = text.starts_with("http://") || text.starts_with("https://") || text.starts_with("www.");
    if looks_web && !text.contains(' ') {
        let url = if text.starts_with("www.") { format!("https://{text}") } else { text.to_string() };
        return Some(Row {
            category: Category::Web,
            name: crate::trf!("Open {address}", address = text),
            detail: String::new(),
            icon: Icon::Glyph('\u{E774}'),
            kind: RowKind::Open(Target::path(url)),
            path: None,
        });
    }
    let is_path = text.len() >= 3 && text.as_bytes()[1] == b':' && (text.as_bytes()[2] == b'\\' || text.as_bytes()[2] == b'/');
    if is_path {
        if let Some((is_dir, _)) = crate::platform::stat(text) {
            let path = text.replace('/', "\\");
            return Some(Row {
                category: Category::Files,
                name: crate::trf!("Open {path}", path = path),
                detail: String::new(),
                icon: Icon::Image { key: crate::icons::file_icon_key(&path, is_dir), source: path.clone() },
                kind: RowKind::Open(Target::path(path.clone())),
                path: Some(path),
            });
        }
    }
    None
}

pub fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, id: &str) -> App {
        serde_json::from_str::<App>(&format!(r#"{{"name":"{name}","id":"{id}","path":""}}"#))
            .map(|mut a| {
                a.lower = matching::lower(&a.name);
                a
            })
            .unwrap()
    }

    #[test]
    fn apps_rank_by_quality_then_by_how_often_they_are_used() {
        let apps = [app("Notes", "a"), app("Notepad", "b"), app("OneNote", "c")];
        let mut cfg = Config { roots: vec![], ..Default::default() };
        let tokens = matching::tokens("note");
        assert_eq!(app_rows(&apps, &cfg, &tokens)[2].name, "OneNote");
        for _ in 0..40 {
            cfg.record_use("OneNote", &Target::App("c".into()));
        }
        assert_eq!(app_rows(&apps, &cfg, &tokens)[0].name, "OneNote");
    }

    #[test]
    fn scattered_letters_only_show_when_nothing_really_matches() {
        let apps = [app("Git Release Notes", "a"), app("Systeminformationen", "b")];
        let cfg = Config { roots: vec![], ..Default::default() };
        let rows = app_rows(&apps, &cfg, &matching::tokens("note"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Git Release Notes");
        let rows = app_rows(&apps[1..], &cfg, &matching::tokens("note"));
        assert_eq!(rows[0].name, "Systeminformationen");
    }

    #[test]
    fn typing_a_setting_finds_its_live_control_first() {
        let rows = setting_rows(&matching::tokens("volume")).rows;
        assert_eq!(rows[0].kind, RowKind::Quick(QuickId::Volume, Want::Nothing));
        let rows = setting_rows(&matching::tokens("brightness")).rows;
        assert_eq!(rows[0].kind, RowKind::Quick(QuickId::Brightness, Want::Nothing));
        let rows = setting_rows(&matching::tokens("wifi")).rows;
        assert_eq!(rows[0].kind, RowKind::Quick(QuickId::Wifi, Want::Nothing));
    }

    #[test]
    fn a_sentence_about_a_setting_gives_its_control_with_the_direction() {
        let found = setting_rows(&matching::tokens("switch to dark mode"));
        assert!(found.lead);
        assert_eq!(found.rows[0].kind, RowKind::Quick(QuickId::DarkMode, Want::On));
        assert_eq!(found.subject, ["dark", "mode"]);
    }

    #[test]
    fn exact_actions_still_win_over_settings_that_sound_alike() {
        assert_eq!(setting_rows(&matching::tokens("sleep")).rows[0].kind, RowKind::Run(Do::Sleep));
        assert_eq!(setting_rows(&matching::tokens("lock screen")).rows[0].kind, RowKind::Run(Do::Lock));
    }

    #[test]
    fn restart_needs_two_clicks_but_lock_does_not() {
        assert!(Do::Restart.needs_confirm());
        assert!(Do::ShutDown.needs_confirm());
        assert!(!Do::Lock.needs_confirm());
    }

    fn row(category: Category, name: &str) -> Row {
        Row { category, name: name.into(), detail: String::new(), icon: Icon::Glyph(' '), kind: RowKind::Open(Target::path(format!("C:\\{name}"))), path: None }
    }

    #[test]
    fn names_fit_whole_then_start_then_word_then_anywhere() {
        assert_eq!(name_fit("Projects", "projects"), 4);
        assert_eq!(name_fit("report.pdf", "Report"), 4, "a file's name without its type counts as whole");
        assert_eq!(name_fit("Project Plan", "proj"), 3);
        assert_eq!(name_fit("Old Projects", "proj"), 2);
        assert_eq!(name_fit("my_projects", "proj"), 2, "after an underscore is a word too");
        assert_eq!(name_fit("Subprojects", "proj"), 1);
        assert_eq!(name_fit("Notepad", "xyz"), 0);
        assert_eq!(name_fit("Notepad", "  "), 0);
    }

    #[test]
    fn the_best_match_is_the_best_name_whatever_its_kind() {
        let order = [Category::Apps, Category::Settings, Category::Files, Category::Web];
        // Apps come first, but the folder typed in full is the best match.
        let rows = [row(Category::Apps, "Microsoft Teams Meetings"), row(Category::Apps, "Team Viewer"), row(Category::Files, "Teams"), row(Category::Web, "Search the web for \"teams\"")];
        assert_eq!(best_match(&rows, "teams", &order, |_| String::new(), |_| 0), Some(2));
        // The same fit: the kind shown first wins, then the one opened most.
        let rows = [row(Category::Files, "notepad.exe"), row(Category::Apps, "Notepad")];
        assert_eq!(best_match(&rows, "notepad", &order, |_| String::new(), |_| 0), Some(1));
        let rows = [row(Category::Files, "Budget"), row(Category::Files, "budget.xlsx")];
        assert_eq!(best_match(&rows, "budget", &order, |_| String::new(), |r| u32::from(r.name == "budget.xlsx") * 5), Some(1));
        assert_eq!(best_match(&rows, "budget", &order, |_| String::new(), |_| 0), Some(0), "else the one listed first");
        // Nothing whose name holds the text: no best match. A web search never is one.
        assert_eq!(best_match(&rows, "zzz", &order, |_| String::new(), |_| 0), None);
        assert_eq!(best_match(&[row(Category::Web, "teams")], "teams", &order, |_| String::new(), |_| 0), None);
        // On German Windows Notepad is called Editor; its program name still makes it the best match for "notepad".
        let rows = [row(Category::Files, "notepad-repl.man"), row(Category::Apps, "Editor")];
        let alias = |r: &Row| if r.name == "Editor" { "notepad notepad".to_string() } else { String::new() };
        assert_eq!(best_match(&rows, "notepad", &order, alias, |_| 0), Some(1));
    }

    #[test]
    fn web_addresses_and_queries_are_encoded() {
        assert_eq!(percent_encode("a b&c"), "a%20b%26c");
        let cfg = Config { roots: vec![], ..Default::default() };
        let row = web_row(" rust lang ", &cfg).unwrap();
        assert_eq!(row.kind, RowKind::Web("rust lang".into()), "the words go to the browser's own search engine");
        assert_eq!(web_address(&cfg, "rust lang"), "https://duckduckgo.com/?q=rust%20lang", "or to the address, when it's asked for or the browser is unknown");
        assert!(matches!(direct_row("https://example.com").unwrap().kind, RowKind::Open(_)));
        assert_eq!(direct_row("not a url"), None);
    }
}
