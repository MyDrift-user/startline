//! The menu's state and behaviour: what happens on a key press, a search or a choice.

use crate::Options;
use crate::actions::{self, Do};
use crate::apps::App;
use crate::config::{Category, Config, PRESETS, Pin, Target};
use crate::customize::Tab;
use crate::icons::{self, IconCache};
use crate::index::{Facets, FileFilter, IndexManager, IndexParams, typed_extension};
use crate::providers::{Ask, Provider, Providers};
use crate::intent::{self, Want};
use crate::pins_page::{self, PinUi};
use crate::popup::Popup;
use crate::scope::{self, Scope};
use crate::search_page::{self, ScopeUi};
use crate::matching;
use crate::platform;
use crate::policy::Policies;
use crate::search::{self, Icon, Row, RowKind};
use crate::settings::{self, Control, QuickId, Settings, Value};
use crate::styles::{self, Experience, Look, SystemLook};
use crate::system::{self, Command, Shape, Shared};
use crate::widgets::*;
use egui::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Page {
    Home,
    AllApps,
    Customize(Tab),
}

#[derive(Clone, Debug)]
pub enum Intent {
    Open { name: String, target: Target, admin: bool },
    Folder(String),
    Copy(String),
    TogglePin { name: String, target: Target },
    Run(Do),
    Web(String),
    /// Opens an address without remembering it among the most used: a message, a meeting, a Teams chat.
    Link(String),
    Show(Page),
    Properties(String),
    OpenWith(String),
    Recycle(String),
    /// Windows' own right-click menu for this item.
    ShellMenu(String),
    MoveSection(usize, i32),
    RemoveSection(usize),
    /// Copies the calculator's answer; the menu stays open for the next sum.
    CopyAnswer(String),
    /// Puts a clipboard history text back on the clipboard, and closes the menu.
    CopyClip(String),
    /// Plays sound on this output from now on.
    UseOutput(String),
    AddBookmark { name: String, target: Target },
    /// Opens one of the person's own bookmarks in Settings, to change it.
    EditBookmark(usize),
    RemoveBookmark(usize),
}

/// What a file or folder dialog was opened for.
#[derive(Clone, Debug)]
pub enum Picked {
    SearchFolder(String),
    Pin(String),
    Bookmark(String),
}

/// The places and rules search should read right now.
fn index_params_of(cfg: &Config) -> IndexParams {
    let autos = search_page::auto_roots(cfg.auto_network, cfg.auto_removable);
    let all = scope::all_roots(cfg, &autos);
    // The top of a drive needs no check, and asking a dead network drive could stall.
    let skip_attrs = cfg.skip_attrs();
    let is_hidden = |path: &str| scope::parent_of(path).is_some() && platform::has_attributes(path, skip_attrs);
    let scope = Scope { roots: &all, excluded_paths: &cfg.excluded_paths, excluded_names: &cfg.exclude, include_hidden: skip_attrs == 0, is_hidden: &is_hidden };
    IndexParams {
        // Only the kinds of drive that are switched on are read or watched at all: local drives, network drives
        // (manually added \\server\share places among them) and USB drives.
        roots: scope.places_to_read().into_iter().filter(|place| cfg.indexes(platform::place_kind(place))).collect(),
        exclude: cfg.exclude.clone(),
        exclude_paths: cfg.excluded_paths.clone(),
        exclude_exts: cfg.excluded_exts.clone(),
        patterns: cfg.skip_patterns.clone(),
        skip_attrs,
    }
}

#[derive(Clone)]
pub(crate) enum NavKind {
    Intent(Intent),
    Quick(QuickId, Want),
}

#[derive(Clone)]
pub(crate) struct NavEntry {
    pub(crate) id: Id,
    pub(crate) rect: Rect,
    pub(crate) kind: NavKind,
}

pub struct Menu {
    pub cfg: Config,
    /// The person's own choices underneath any policy, which is what gets saved.
    own_cfg: Config,
    pub(crate) policy: Arc<Policies>,
    /// Policies the watcher read while Startline runs, waiting to be taken in.
    policy_slot: Arc<Mutex<Option<Policies>>>,
    applied: Option<Config>,
    pub(crate) system_look: SystemLook,
    pub look: Look,
    look_shown: Option<Look>,
    glass_shown: Option<(bool, [u8; 3], f32, bool)>,
    changed_at: Option<Instant>,
    pub shared: Arc<Shared>,
    pub(crate) apps: Arc<RwLock<Vec<App>>>,
    apps_seen: usize,
    pub index: IndexManager,
    pub index_params: IndexParams,
    pub(crate) providers: Providers,
    pub(crate) icons: IconCache,
    pub settings: Settings,
    pub(crate) recent: Arc<Mutex<Vec<(String, String, u32)>>>,

    pub page: Page,
    pub(crate) query: String,
    pub(crate) tokens: Vec<String>,
    pub(crate) subject: Vec<String>,
    /// The file type buttons switched on under the search bar.
    pub(crate) file_filter: FileFilter,
    pub(crate) facets: Facets,
    pub(crate) file_micros: Option<u64>,
    pub(crate) fast_micros: u64,
    pub(crate) rows: Vec<Row>,
    /// Which of `rows` is the best match, shown first under a heading of its own.
    pub(crate) best_at: Option<usize>,
    /// Why a regular expression in the search couldn't be used.
    pub(crate) search_error: Option<String>,
    /// The panel that shows what can be typed in the search box.
    pub(crate) help_open: bool,

    pub(crate) selected: Option<Id>,
    pub(crate) scroll_to_selected: bool,
    /// The person moved to the selected row themselves, so left and right are for it, not
    /// the text cursor. A wrong arrow while fixing a typo must not change a setting.
    nav_by_keys: bool,
    pub(crate) nav: Vec<NavEntry>,
    pub(crate) nav_next: Vec<NavEntry>,
    pub intents: Vec<Intent>,
    pub(crate) confirm: Option<(Do, Instant)>,
    pub(crate) drag_pin: Option<(usize, usize)>,
    pub(crate) popup: Option<Popup>,
    /// The settings window, a real window of its own beside the Start menu.
    pub(crate) settings_open: bool,
    pub(crate) settings_tab: Tab,
    pub(crate) settings_raise: bool,
    pub(crate) settings_start: ([f32; 2], Option<[f32; 2]>),
    pub(crate) drawing_settings: bool,
    pub(crate) user_picture: Arc<Mutex<Option<String>>>,
    picked: Arc<Mutex<Option<Picked>>>,
    index_dirty_at: Option<Instant>,

    // The Settings pages' own state.
    pub pin_ui: PinUi,
    pub scope_ui: ScopeUi,
    pub reset_armed: Option<Instant>,

    /// The Microsoft 365 account, while one is signed in and switched on.
    pub(crate) m365: Option<Arc<crate::m365::Session>>,
    pub(crate) m365_ui: crate::m365::page::M365Ui,
    pub(crate) calendar: crate::m365::panel::Calendar,
    /// Whether the Teams app takes msteams: links, so chats and meetings open in it.
    pub(crate) teams_installed: bool,
    /// What's playing, while the setting is on.
    pub(crate) media: crate::media::Media,
    pub(crate) bookmark_ui: crate::bookmarks_page::BookmarkUi,
    /// Sites' pictures made ready to draw, with the reading of the browsers they come from.
    pub(crate) favicons: (u64, std::collections::HashMap<String, Option<egui::TextureHandle>>),
    /// Mica: the desktop picture, blurred and tinted, made ready to draw behind the menu.
    pub(crate) mica: crate::mica::Material,

    /// Not running for pictures and tests.
    pub(crate) updater: Option<crate::update::Updater>,
    auto_update: Arc<AtomicBool>,
    /// The person chose to update now, rather than when the menu is next out of sight.
    pub(crate) update_requested: bool,

    pub(crate) options: Options,
    frames: u64,
    started: Instant,
    shot_requested: bool,
    hide_after_frame: bool,
    pub(crate) focus_search: bool,
}

impl Menu {
    pub fn new(cc: &eframe::CreationContext<'_>, mut cfg: Config, options: Options) -> Menu {
        let ctx = cc.egui_ctx.clone();

        let screenshot = options.screenshot.is_some();
        if let Some(experience) = options.experience.as_deref().and_then(Experience::from_name) {
            cfg.experience = experience;
        }
        // Company policies come first: nothing below should see a setting they overrule.
        let own_cfg = cfg.clone();
        let policy = Arc::new(read_policies(&options));
        policy.enforce(&mut cfg);
        crate::i18n::set(crate::i18n::Lang::from_setting(&cfg.language));
        install_fonts(&ctx, cfg.experience);
        // On from the very first run, and kept pointing at wherever this copy now lives.
        if !screenshot && !options.no_hook && !options.test_injected_keys {
            if policy.autostart().is_some() {
                policy.apply_autostart();
            } else if !cfg.autostart_applied {
                // The first run: on, unless an administrator's default says otherwise.
                settings::set_autostart(policy.first_autostart().unwrap_or(true));
                cfg.autostart_applied = true;
                policy.without_forced(&cfg, &own_cfg).save();
            } else if settings::autostart_enabled() {
                settings::set_autostart(true);
            }
        }
        let system_look = SystemLook::read();
        let look = styles::look_of(&cfg, system_look);
        if let Some(preset) = &options.preset {
            if let Some((name, theme)) = PRESETS.iter().find(|(name, _)| name.eq_ignore_ascii_case(preset)) {
                cfg.preset = name.to_string();
                cfg.theme = *theme;
            }
        }

        let shared = Shared::new(styles::placement_of(&cfg, system_look, false), cfg.take_over_start, cfg.take_over_search);
        shared.accept_injected.store(options.test_injected_keys, Ordering::SeqCst);
        if let Some(hwnd) = window_handle(cc) {
            shared.attach(hwnd, ctx.clone());
        }
        // The check run only needs a picture: it leaves the keyboard and tray alone.
        if !screenshot {
            system::start(shared.clone(), !options.no_hook);
            shared.place_window(false);
        }

        let index_params = index_params_of(&cfg);
        let index = IndexManager::start(index_params.clone());
        let apps = crate::apps::start(ctx.clone());
        let providers = Providers::start(apps.clone(), index.index.clone(), ctx.clone());
        let icons = IconCache::new(ctx.clone());
        let settings = Settings::start(ctx.clone());
        let recent: Arc<Mutex<Vec<(String, String, u32)>>> = Arc::default();

        let page = match options.page.as_deref() {
            Some("allapps") => Page::AllApps,
            Some(name) => Tab::from_name(name).map_or(Page::Home, Page::Customize),
            None => Page::Home,
        };
        let mut menu = Menu {
            cfg,
            own_cfg,
            policy,
            policy_slot: Arc::default(),
            applied: None,
            system_look,
            look,
            look_shown: None,
            glass_shown: None,
            changed_at: None,
            shared,
            apps,
            apps_seen: 0,
            index,
            index_params,
            providers,
            icons,
            settings,
            recent,
            page,
            query: String::new(),
            tokens: Vec::new(),
            subject: Vec::new(),
            file_filter: FileFilter::default(),
            facets: Facets::default(),
            file_micros: None,
            fast_micros: 0,
            rows: Vec::new(),
            best_at: None,
            search_error: None,
            help_open: false,
            selected: None,
            scroll_to_selected: false,
            nav_by_keys: false,
            nav: Vec::new(),
            nav_next: Vec::new(),
            intents: Vec::new(),
            confirm: None,
            drag_pin: None,
            popup: None,
            settings_open: false,
            settings_tab: Tab::Appearance,
            settings_raise: false,
            settings_start: ([1040.0, 720.0], None),
            drawing_settings: false,
            user_picture: Arc::default(),
            picked: Arc::default(),
            index_dirty_at: None,
            pin_ui: PinUi::default(),
            scope_ui: ScopeUi::default(),
            reset_armed: None,
            m365: None,
            m365_ui: crate::m365::page::M365Ui::default(),
            calendar: crate::m365::panel::Calendar::default(),
            teams_installed: crate::m365::teams_installed(),
            media: crate::media::Media::start(ctx.clone()),
            bookmark_ui: Default::default(),
            favicons: (0, std::collections::HashMap::new()),
            mica: Default::default(),
            updater: None,
            auto_update: Arc::new(AtomicBool::new(true)),
            update_requested: false,
            hide_after_frame: options.background,
            options,
            frames: 0,
            started: Instant::now(),
            shot_requested: false,
            focus_search: true,
        };
        menu.share_limits();
        if !screenshot {
            crate::policy::watch(ctx.clone(), menu.policy_slot.clone(), (*menu.policy).clone());
        }
        menu.refresh_recent();
        if !screenshot && !menu.options.test_injected_keys {
            menu.auto_update.store(menu.cfg.auto_update, Ordering::Relaxed);
            menu.updater = Some(crate::update::Updater::start(ctx.clone(), menu.auto_update.clone()));
        }
        let (slot, repaint) = (menu.user_picture.clone(), ctx.clone());
        std::thread::spawn(move || {
            platform::com_init();
            *slot.lock().unwrap() = platform::account_picture();
            repaint.request_repaint();
        });
        if menu.options.demo_open {
            menu.scope_ui.open_home(&ctx);
            menu.pin_ui = PinUi::with_first_selected();
        }
        // Only ever for pictures: a made-up account that never signs in or sends anything.
        if menu.options.demo_m365 && menu.is_shot() {
            menu.cfg.m365 = true;
            menu.cfg.m365_calendar = true;
            menu.cfg.m365_people = true;
            if !crate::m365::available(&menu.cfg) {
                menu.cfg.m365_client_id = "00000000-0000-0000-0000-000000000001".into();
            }
            menu.cfg.m365_account_id = "demo".into();
            menu.calendar.demo();
            let me = crate::m365::graph::Me { id: "demo".into(), name: "Alex Wilber".into(), mail: "alex@contoso.com".into() }; // i18n-ignore: an example account for pictures
            menu.m365_ui.show_demo(me);
            menu.shared.set_placement(styles::placement_of(&menu.cfg, menu.system_look, false));
        }
        if menu.options.demo_media && menu.is_shot() {
            menu.cfg.media_controls = true;
            menu.media.show_demo();
        }
        if let Some(query) = menu.options.query.clone() {
            menu.query = query;
            menu.query_changed();
            if menu.options.demo_m365 && menu.is_shot() {
                let person = |name: &str, title: &str, upn: &str| crate::m365::graph::Person { name: name.into(), title: title.into(), mail: upn.into(), upn: upn.into() };
                let people = vec![
                    person(&format!("{} Martin", menu.query.trim()), "Product Manager", "alex.martin@contoso.com"), // i18n-ignore: example people for pictures
                    person(&format!("{} Rivera", menu.query.trim()), "Designer", "sam.rivera@contoso.com"), // i18n-ignore: example people for pictures
                ];
                menu.providers.people.show(crate::providers::people_rows(people));
                menu.assemble_rows();
            }
        }
        menu
    }

    pub fn refresh_recent(&self) {
        let (slot, count) = (self.recent.clone(), self.cfg.recent_count.max(1));
        std::thread::spawn(move || {
            platform::com_init();
            let list = platform::recent_files(count);
            *slot.lock().unwrap() = list;
        });
    }

    pub(crate) fn is_shot(&self) -> bool {
        self.options.screenshot.is_some()
    }

    pub(crate) fn pal(&self) -> Pal {
        palette(&self.look)
    }

    pub fn file_count(&self) -> usize {
        self.index.index.read().map(|i| i.len()).unwrap_or(0)
    }

    // ---- Search flow ----

    pub(crate) fn query_changed(&mut self) {
        self.tokens = matching::tokens(&self.query);
        self.selected = None;
        self.nav_by_keys = false;
        if self.query.trim().is_empty() {
            self.subject.clear();
            self.file_filter = FileFilter::default();
            self.facets = Facets::default();
            self.file_micros = None;
            for provider in self.providers.each_mut() {
                provider.clear();
            }
            self.rows.clear();
            self.best_at = None;
            return;
        }
        self.ask_providers(None);
    }

    /// Tells the providers about the search, every one of them or only those of one kind, and lists what they have.
    pub(crate) fn ask_providers(&mut self, only: Option<Category>) {
        let started = Instant::now();
        let filter = FileFilter { regex: self.regex_on(), ..self.file_filter.clone() };
        let ask = Ask { query: &self.query, tokens: &self.tokens, cfg: &self.cfg, policy: &self.policy, filter: &filter, m365: self.m365.as_ref() };
        for provider in self.providers.each_mut() {
            if only.is_none_or(|kind| provider.category() == kind) {
                provider.ask(&ask);
            }
        }
        self.subject = self.providers.settings.subject.clone();
        self.fast_micros = started.elapsed().as_micros() as u64;
        self.assemble_rows();
    }

    /// Answers that came in the background, from files and Microsoft 365.
    fn poll_providers(&mut self) {
        let mut changed = false;
        for provider in self.providers.each_mut() {
            changed |= provider.poll();
        }
        if changed {
            let files = &self.providers.files;
            self.facets = files.facets.clone();
            self.file_micros = files.micros;
            self.search_error = files.error.clone();
            self.assemble_rows();
        }
    }

    /// File types typed into the search itself: ".pdf", "*.pdf", "ext:pdf".
    pub(crate) fn typed_filters(&self) -> Vec<String> {
        self.tokens.iter().filter_map(|word| typed_extension(word)).collect()
    }

    /// Whether the search is after files only: a file type button, a typed type, the search language or regex mode.
    pub(crate) fn filtering(&self) -> bool {
        !self.file_filter.is_empty() || !self.typed_filters().is_empty() || self.regex_on() || (crate::index::uses_file_syntax(&self.query) && !self.is_sum())
    }

    /// A sum for the calculator, such as 12*7+3, whose * isn't a file name wildcard.
    fn is_sum(&self) -> bool {
        self.cfg.quick_calculator && self.cfg.shows(Category::Actions) && crate::calc::answer(&self.query).is_some()
    }

    /// Regex mode, which only counts while its switch is there to turn it off again, unless a policy fixes it on.
    pub(crate) fn regex_on(&self) -> bool {
        self.cfg.regex_search && (self.cfg.show_regex_switch || self.policy.is_set("SearchRegex"))
    }

    /// A file type button was pressed. A type that was typed gets taken out of the words.
    pub(crate) fn toggle_ext_filter(&mut self, ext: &str) {
        if self.typed_filters().iter().any(|typed| typed == ext) {
            let kept: Vec<&str> = self.query.split_whitespace().filter(|word| typed_extension(&matching::lower(word)).as_deref() != Some(ext)).collect();
            self.query = kept.join(" ");
        } else if let Some(at) = self.file_filter.exts.iter().position(|e| e == ext) {
            self.file_filter.exts.remove(at);
        } else {
            self.file_filter.exts.push(ext.to_string());
        }
        self.query_changed();
    }

    pub(crate) fn toggle_kind_filter(&mut self, exts: &[&str]) {
        let all_on = exts.iter().all(|ext| self.file_filter.exts.iter().any(|e| e == ext));
        self.file_filter.exts.retain(|e| !exts.contains(&e.as_str()));
        if !all_on {
            self.file_filter.exts.extend(exts.iter().map(|e| e.to_string()));
        }
        self.query_changed();
    }

    pub(crate) fn toggle_folder_filter(&mut self) {
        self.file_filter.folders = !self.file_filter.folders;
        self.query_changed();
    }

    pub(crate) fn clear_filters(&mut self) {
        self.file_filter = FileFilter::default();
        let kept: Vec<&str> = self.query.split_whitespace().filter(|word| typed_extension(&matching::lower(word)).is_none()).collect();
        self.query = kept.join(" ");
        self.query_changed();
    }

    /// Opens the Windows picker in front of the window, on a helper thread.
    pub fn ask_for_path(&self, ctx: &Context, folder: bool, make: fn(String) -> Picked) {
        let (slot, owner, ctx) = (self.picked.clone(), self.shared.window_id(), ctx.clone());
        std::thread::spawn(move || {
            platform::com_init();
            if let Some(path) = platform::pick_path(owner, folder) {
                *slot.lock().unwrap() = Some(make(path));
                ctx.request_repaint();
            }
        });
    }

    /// Adds a place to search, unless it is already reached through another one.
    pub fn add_search_place(&mut self, place: String) {
        let already = {
            let autos = search_page::auto_roots(self.cfg.auto_network, self.cfg.auto_removable);
            let all = scope::all_roots(&self.cfg, &autos);
            let scope = Scope { roots: &all, excluded_paths: &self.cfg.excluded_paths, excluded_names: &self.cfg.exclude, include_hidden: self.cfg.skip_attrs() == 0, is_hidden: &|_| false };
            scope.is_searched(&place)
        };
        self.cfg.excluded_paths.retain(|p| !scope::same_path(p, &place));
        if !already {
            self.cfg.roots.push(place);
        }
    }

    pub(crate) fn assemble_rows(&mut self) {
        let direct = search::direct_row(&self.query);
        let mut rows: Vec<Row> = Vec::new();
        self.best_at = None;
        rows.extend(direct);
        // A file type filter means the person is after files: nothing else belongs in the list.
        if self.filtering() {
            if self.cfg.shows(Category::Files) {
                rows.extend(self.providers.files.rows(&self.cfg));
            }
            self.rows = rows;
            return;
        }
        // A provider the search is plainly about goes first, then the rest in the order the person chose.
        let lead = self.providers.each().into_iter().find(|p| p.leads() && self.cfg.shows(p.category())).map(|p| p.category());
        let mut order: Vec<Category> = lead.into_iter().collect();
        order.extend(self.cfg.results.iter().filter(|r| r.on).map(|r| r.kind).filter(|k| Some(*k) != lead));
        let mut listed: Vec<Row> = Vec::new();
        for &category in &order {
            listed.extend(self.providers.of(category).rows(&self.cfg));
        }
        // A PC without a Wi-Fi or Bluetooth radio isn't offered a switch for it.
        listed.retain(|row| match row.kind {
            RowKind::Quick(id @ (QuickId::Wifi | QuickId::Bluetooth), _) => self.settings.get(id) != Value::Unavailable,
            _ => true,
        });
        // The one result whose name fits best goes first, whatever its kind, and isn't listed again below.
        if self.cfg.best_match && !self.regex_on() {
            let uses = |row: &Row| match &row.kind {
                RowKind::Open(target) => self.cfg.usage.get(&target.key()).map_or(0, |u| u.count),
                _ => 0,
            };
            let apps = self.apps.read().unwrap();
            let other_names = |row: &Row| match &row.kind {
                RowKind::Open(Target::App(id)) => apps.iter().find(|app| &app.id == id).map(|app| app.alias.clone()).unwrap_or_default(),
                _ => String::new(),
            };
            let best = search::best_match(&listed, &self.query, &order, other_names, uses);
            drop(apps);
            if let Some(at) = best {
                self.best_at = Some(rows.len());
                rows.push(listed.remove(at));
            }
        }
        rows.extend(listed);
        self.rows = rows;
    }

    // ---- Applying what the person chose ----

    fn apply(&mut self, intent: Intent) {
        if !self.policy.allows_intent(&intent) {
            return;
        }
        match intent {
            Intent::Open { name, target, admin } => {
                platform::open_target(&target, admin);
                self.cfg.record_use(&name, &target);
                self.close_for_launch();
            }
            Intent::Folder(path) => {
                platform::show_in_folder(&path);
                self.close_for_launch();
            }
            Intent::Copy(text) => {
                let _ = platform::copy_text(&text);
            }
            Intent::TogglePin { name, target } => self.cfg.toggle_pin(&name, &target),
            Intent::Link(url) => {
                platform::open_async("", url, String::new());
                self.close_for_launch();
            }
            Intent::Web(words) => {
                let address = search::web_address(&self.cfg, &words);
                if self.cfg.web_fixed {
                    platform::open_async("", address, String::new());
                } else {
                    crate::browser::search(words, address);
                }
                self.close_for_launch();
            }
            Intent::Show(Page::Customize(tab)) if !self.is_shot() => self.open_settings(tab),
            Intent::Show(page) => {
                self.leave_customize_if_needed(page);
                self.page = page;
                self.selected = None;
                self.focus_search = true;
            }
            Intent::Run(what) => self.run(what),
            Intent::Properties(path) => {
                platform::show_properties(path);
                self.close_for_launch();
            }
            Intent::OpenWith(path) => {
                platform::open_with(path);
                self.close_for_launch();
            }
            Intent::Recycle(path) => {
                platform::recycle(path);
                self.close_for_launch();
            }
            Intent::ShellMenu(source) => {
                // The Windows menu takes the focus, which must not close this one underneath it.
                self.shared.hold_open.store(true, Ordering::SeqCst);
                let shared = self.shared.clone();
                std::thread::spawn(move || {
                    let _ = platform::shell_context_menu(&source);
                    shared.hold_open.store(false, Ordering::SeqCst);
                    shared.hide_for_launch();
                });
            }
            Intent::MoveSection(group, step) => {
                let to = group as i32 + step;
                if to >= 0 && (to as usize) < self.cfg.groups.len() {
                    self.cfg.groups.swap(group, to as usize);
                }
            }
            Intent::RemoveSection(group) => {
                // Its pins stay: they move to the first remaining section.
                if self.cfg.groups.len() > 1 && group < self.cfg.groups.len() {
                    let removed = self.cfg.groups.remove(group);
                    if let Some(first) = self.cfg.groups.first_mut() {
                        first.pins.extend(removed.pins);
                    }
                }
            }
            Intent::CopyAnswer(text) => {
                let _ = platform::copy_text(&text);
            }
            Intent::CopyClip(text) => {
                let _ = platform::copy_text(&text);
                self.close_menu();
            }
            Intent::UseOutput(id) => {
                self.providers.quick.use_output(id);
                self.close_menu();
            }
            Intent::AddBookmark { name, target } => {
                if !self.cfg.bookmarks.iter().any(|b| b.target == target) {
                    self.cfg.bookmarks.push(crate::bookmarks::Bookmark { folder: String::new(), name, target });
                }
                self.assemble_after_change();
            }
            Intent::EditBookmark(at) => {
                self.bookmark_ui.edit(&self.cfg, Some(at));
                self.open_settings(Tab::Search);
            }
            Intent::RemoveBookmark(at) => {
                if at < self.cfg.bookmarks.len() {
                    self.cfg.bookmarks.remove(at);
                }
                self.assemble_after_change();
            }
        }
    }

    /// The results again after something they show changed.
    pub(crate) fn assemble_after_change(&mut self) {
        if !self.query.trim().is_empty() {
            self.ask_providers(Some(Category::Bookmarks));
        }
    }

    /// Opens the settings window, or brings it forward if it's already open.
    fn open_settings(&mut self, tab: Tab) {
        if !self.policy.allows_do(Do::Customize) {
            return;
        }
        self.settings_tab = tab;
        if !self.settings_open {
            self.settings_start = (self.cfg.settings_size, self.cfg.settings_pos);
        }
        self.settings_open = true;
        self.settings_raise = true;
        if self.shared.visible.load(Ordering::SeqCst) {
            self.close_for_launch();
        }
    }

    pub(crate) fn frames(&self) -> u64 {
        self.frames
    }

    fn run(&mut self, what: Do) {
        if !self.policy.allows_do(what) {
            return;
        }
        if what.needs_confirm() {
            let armed = self.confirm.is_some_and(|(d, at)| d == what && at.elapsed() < Duration::from_secs(5));
            if !armed {
                self.confirm = Some((what, Instant::now()));
                return;
            }
        }
        self.confirm = None;
        match what {
            Do::Customize => self.apply(Intent::Show(Page::Customize(Tab::Appearance))),
            Do::PinPage => self.apply(Intent::Show(Page::Customize(Tab::Pins))),
            Do::RealStart => self.shared.open_native_start(),
            Do::Quit => {
                self.save_settings();
                system::shutdown();
            }
            other => {
                actions::run_system(other);
                self.close_for_launch();
            }
        }
    }

    fn close_menu(&mut self) {
        if self.is_shot() {
            return;
        }
        self.leave_customize_if_needed(Page::Home);
        self.save_settings();
        self.changed_at = None;
        self.shared.hide();
    }

    /// Something new is opening, so its window gets the focus rather than the previous one.
    fn close_for_launch(&mut self) {
        if self.is_shot() {
            return;
        }
        self.leave_customize_if_needed(Page::Home);
        self.save_settings();
        self.changed_at = None;
        self.shared.hide_for_launch();
    }

    /// A test run must not leave its mark on the person's real settings.
    fn save_settings(&self) {
        if !self.options.test_injected_keys {
            self.policy.without_forced(&self.cfg, &self.own_cfg).save();
        }
    }

    /// Installs a downloaded update when the person asked for it, or by itself once nothing of Startline is on screen.
    /// Startline then ends, and the installer or the new copy starts it again.
    fn drive_update(&mut self, ctx: &Context) {
        let Some(updater) = &self.updater else { return };
        self.auto_update.store(self.cfg.auto_update, Ordering::Relaxed);
        if !matches!(updater.status(), crate::update::Status::Ready(_)) {
            return;
        }
        let quiet = !self.shared.visible.load(Ordering::SeqCst) && !self.settings_open;
        if !(self.update_requested || (self.cfg.auto_update && quiet)) {
            // Nothing else may wake the app once the menu is closed.
            ctx.request_repaint_after(Duration::from_secs(30));
            return;
        }
        self.update_requested = false;
        self.save_settings();
        match updater.finish() {
            Ok(()) => system::shutdown(),
            Err(e) => updater.fail(e),
        }
    }

    /// Tells the tray what policies allow.
    fn share_limits(&self) {
        let policy = &self.policy;
        self.shared.tray_fixed_start.store(policy.is_set("TakeOverStartKey"), Ordering::Relaxed);
        self.shared.tray_no_settings.store(!policy.allows_do(Do::Customize), Ordering::Relaxed);
        self.shared.tray_no_native.store(!policy.allows_do(Do::RealStart), Ordering::Relaxed);
        self.shared.tray_no_quit.store(!policy.allows_do(Do::Quit), Ordering::Relaxed);
    }

    /// Picks up policy changes, and puts back anything that moved away from a policy's value.
    fn keep_to_policies(&mut self) {
        // Settings the person changed since the last frame stop following an administrator's default.
        if let Some(before) = &self.applied {
            if *before != self.cfg {
                self.policy.note_choices(&mut self.cfg, before);
            }
        }
        let read = self.policy_slot.lock().unwrap().take();
        if let Some(fresh) = read {
            if fresh != *self.policy {
                self.own_cfg = self.policy.without_forced(&self.cfg, &self.own_cfg);
                self.cfg = self.own_cfg.clone();
                self.policy = Arc::new(fresh);
                self.share_limits();
                if !self.options.no_hook && !self.options.test_injected_keys {
                    self.policy.apply_autostart();
                }
                self.policy.enforce(&mut self.cfg);
                self.shared.take_start.store(self.cfg.take_over_start, Ordering::Relaxed);
                self.shared.take_search.store(self.cfg.take_over_search, Ordering::Relaxed);
                if !self.query.trim().is_empty() {
                    self.ask_providers(None);
                }
            }
        }
        if !self.policy.is_empty() && self.applied.as_ref() != Some(&self.cfg) {
            self.policy.enforce(&mut self.cfg);
            self.shared.take_start.store(self.cfg.take_over_start, Ordering::Relaxed);
            self.shared.take_search.store(self.cfg.take_over_search, Ordering::Relaxed);
        }
    }

    fn leave_customize_if_needed(&mut self, next: Page) {
        if matches!(self.page, Page::Customize(_)) && !matches!(next, Page::Customize(_)) {
            self.sync_index();
        }
    }

    /// If the searched folders changed, read them again.
    pub fn sync_index(&mut self) {
        let params = index_params_of(&self.cfg);
        if params != self.index_params {
            self.index_params = params.clone();
            self.index.reindex(params);
        }
    }

    /// Start key, Start button or Start + S: one menu, with search always ready in it.
    fn opened(&mut self) {
        self.query.clear();
        self.query_changed();
        self.page = Page::Home;
        self.system_look = SystemLook::read();
        // A network or USB drive may have come or gone since the last time.
        if !self.is_shot() && (self.cfg.auto_network || self.cfg.auto_removable) {
            self.sync_index();
        }
        self.selected = None;
        self.popup = None;
        self.confirm = None;
        self.focus_search = true;
        self.calendar.opened();
        self.settings.refresh();
        self.refresh_recent();
    }

    fn seed_pins(&mut self) {
        if self.cfg.pins_seeded {
            return;
        }
        if self.policy.sets_pins() {
            self.cfg.pins_seeded = true;
            return;
        }
        let apps = self.apps.read().unwrap();
        if apps.is_empty() {
            return;
        }
        // Matched on the app's id, so it works in every Windows language.
        const WANTED: &[&str] = &[
            "microsoft.windows.explorer",
            "immersivecontrolpanel",
            "msedge",
            "chrome",
            "firefox",
            "windowsterminal",
            "windowsnotepad",
            "windowscalculator",
            "paint",
            "photos",
            "screensketch",
            "taskmgr",
            "windowsstore",
            "outlook",
            "spotify",
            "code",
        ];
        let mut pins = Vec::new();
        for wanted in WANTED {
            if pins.len() >= 12 {
                break;
            }
            let found = apps.iter().find(|a| {
                let id = matching::lower(&a.id);
                let path = matching::lower(&a.path);
                let stem = path.rsplit('\\').next().unwrap_or("");
                id.contains(wanted) || stem.starts_with(wanted)
            });
            if let Some(app) = found {
                let target = Target::App(app.id.clone());
                if !pins.iter().any(|p: &Pin| p.target == target) {
                    pins.push(Pin { name: app.name.clone(), target });
                }
            }
        }
        drop(apps);
        if let Some(group) = self.cfg.groups.first_mut() {
            if group.pins.is_empty() {
                group.pins = pins;
            }
        }
        self.cfg.pins_seeded = true;
    }

    /// Reacts to anything that changed in the settings, however it was changed.
    fn apply_config_side_effects(&mut self, ctx: &Context) {
        let in_settings = matches!(self.page, Page::Customize(_));
        self.look = styles::look_of(&self.cfg, self.system_look);
        let mut placement = styles::placement_of(&self.cfg, self.system_look, in_settings);
        if self.options.demo_open {
            placement.height = 1100.0;
        }
        self.shared.set_placement(placement);
        if self.look_shown != Some(self.look) {
            let old = self.look_shown;
            if old.is_some_and(|o| o.experience != self.look.experience) {
                install_fonts(ctx, self.look.experience);
            }
            apply_style(ctx, &self.look);
            self.shared.set_shape(Shape {
                corner: self.look.corner * self.look.ui_scale,
                round_bottom: self.look.round_bottom,
                native: self.look.native_corner,
            });
            self.look_shown = Some(self.look);
        }
        // Settings is a solid window, except in Startline where the glass is a choice.
        let style = self.look.experience;
        let glass_on = self.look.blur && (!in_settings || style == Experience::Startline);
        // Windows 7's window color is its white list; its glass is blue.
        let (tint, strength) = if style == Experience::Windows7 { ([28, 66, 104], 0.5) } else { (self.look.theme.bg, (self.look.opacity - 0.25).max(0.0)) };
        // Mica takes the place of the glass. The menu paints it itself (see mica.rs), so Windows' glass is off under it.
        let mica = self.look.mica && (!in_settings || style == Experience::Startline);
        let glass = (glass_on && !mica, tint, strength, mica);
        if self.glass_shown != Some(glass) {
            self.shared.set_blur(glass.0, tint, strength);
            self.glass_shown = Some(glass);
        }
        if self.applied.as_ref() == Some(&self.cfg) {
            return;
        }
        let first = self.applied.is_none();
        self.shared.take_start.store(self.cfg.take_over_start, Ordering::Relaxed);
        self.shared.take_search.store(self.cfg.take_over_search, Ordering::Relaxed);
        if !first {
            self.changed_at = Some(Instant::now());
            self.index_dirty_at = Some(Instant::now());
            if !self.query.is_empty() {
                self.assemble_rows();
            }
        }
        self.applied = Some(self.cfg.clone());
    }

    // ---- Keyboard ----

    fn handle_keys(&mut self, ctx: &Context) {
        if self.popup_keys(ctx) || crate::widgets::text_menu_open(ctx) {
            return;
        }
        let in_customize = matches!(self.page, Page::Customize(_));
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            if !self.query.is_empty() {
                self.query.clear();
                self.query_changed();
            } else if self.page != Page::Home {
                self.apply(Intent::Show(Page::Home));
            } else {
                self.close_menu();
            }
        }
        if in_customize {
            return;
        }
        let (up, down, tab, back_tab) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Tab),
                i.consume_key(Modifiers::SHIFT, Key::Tab),
            )
        });
        if up || down || tab || back_tab {
            self.nav_by_keys = true;
        }
        let selected_quick = self.selected_entry().and_then(|e| match e.kind {
            NavKind::Quick(id, _) => Some(id),
            _ => None,
        });
        // Left and right belong to the text cursor once there is text, unless the person moved to a control.
        let arrows_are_ours = self.query.is_empty() || (selected_quick.is_some() && self.nav_by_keys);
        let (mut left, mut right) = (false, false);
        if arrows_are_ours {
            ctx.input_mut(|i| {
                left = i.consume_key(Modifiers::NONE, Key::ArrowLeft);
                right = i.consume_key(Modifiers::NONE, Key::ArrowRight);
            });
        }
        if let Some(id) = selected_quick {
            if left || right {
                self.nudge_quick(id, if right { 1.0 } else { -1.0 });
                left = false;
                right = false;
            }
        }
        if up {
            self.move_selection(vec2(0.0, -1.0));
        }
        if down {
            self.move_selection(vec2(0.0, 1.0));
        }
        if left {
            self.move_selection(vec2(-1.0, 0.0));
        }
        if right {
            self.move_selection(vec2(1.0, 0.0));
        }
        if tab || back_tab {
            self.step_selection(if tab { 1 } else { -1 });
        }

        let (enter, ctrl_enter, shift_enter) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::CTRL, Key::Enter),
                i.consume_key(Modifiers::SHIFT, Key::Enter),
            )
        });
        if enter || ctrl_enter || shift_enter {
            if let Some(entry) = self.selected_entry().or_else(|| self.nav.first().cloned().filter(|_| !self.query.is_empty())) {
                match entry.kind {
                    NavKind::Quick(id, want) => self.activate_quick(id, want),
                    NavKind::Intent(Intent::Open { name, target, .. }) if ctrl_enter => {
                        self.apply(Intent::Open { name, target, admin: true });
                    }
                    NavKind::Intent(Intent::Open { target, name, admin }) if shift_enter => match self.path_of_target(&target) {
                        Some(path) => self.apply(Intent::Folder(path)),
                        None => self.apply(Intent::Open { name, target, admin }),
                    },
                    NavKind::Intent(intent) => self.apply(intent),
                }
            }
        }
    }

    pub(crate) fn path_of_target(&self, target: &Target) -> Option<String> {
        match target {
            Target::Path { path, .. } if !path.contains("://") => Some(path.clone()),
            Target::Path { .. } => None,
            Target::App(id) => {
                let apps = self.apps.read().unwrap();
                apps.iter().find(|a| &a.id == id).map(|a| a.path.clone()).filter(|p| !p.is_empty())
            }
        }
    }

    pub(crate) fn selected_entry(&self) -> Option<NavEntry> {
        let id = self.selected?;
        self.nav.iter().find(|n| n.id == id).cloned()
    }

    fn move_selection(&mut self, direction: Vec2) {
        let Some(current) = self.selected_entry() else {
            self.selected = self.nav.first().map(|n| n.id);
            self.scroll_to_selected = true;
            return;
        };
        let from = current.rect.center();
        let best = self
            .nav
            .iter()
            .filter(|n| n.id != current.id)
            .filter_map(|n| {
                let delta = n.rect.center() - from;
                let along = delta.dot(direction);
                if along <= 4.0 {
                    return None;
                }
                let across = (delta - direction * along).length();
                Some((along + across * 2.5, n.id))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, id)) = best {
            self.selected = Some(id);
            self.scroll_to_selected = true;
        }
    }

    fn step_selection(&mut self, step: i32) {
        if self.nav.is_empty() {
            return;
        }
        let at = self.selected.and_then(|id| self.nav.iter().position(|n| n.id == id));
        let next = match at {
            Some(i) => (i as i32 + step).rem_euclid(self.nav.len() as i32) as usize,
            None => 0,
        };
        self.selected = Some(self.nav[next].id);
        self.scroll_to_selected = true;
    }

    /// Does what the search asked for. The menu stays open so the result can be seen.
    pub(crate) fn activate_quick(&mut self, id: QuickId, want: Want) {
        let info = id.info();
        let now = self.settings.get(id);
        if now == Value::Unavailable {
            platform::open_async("", info.page.into(), String::new());
        } else if let Some(next) = intent::resolve(info.control, now, want) {
            self.settings.set(id, next);
        }
    }

    fn nudge_quick(&mut self, id: QuickId, direction: f32) {
        match (id.info().control, self.settings.get(id)) {
            (Control::Slider { min, max, step, .. }, Value::Number(value)) => {
                let jump = if step >= 1.0 && max > 50.0 { 5.0 } else { step };
                self.settings.set(id, Value::Number((value + jump * direction).clamp(min, max)));
            }
            (Control::Choice(options), Value::Pick(i)) => {
                let next = (i as i32 + direction as i32).clamp(0, options.len() as i32 - 1) as usize;
                self.settings.set(id, Value::Pick(next));
            }
            (Control::Toggle, Value::On(_)) => self.settings.set(id, Value::On(direction > 0.0)),
            _ => {}
        }
    }

    fn screenshot_tick(&mut self, ctx: &Context) {
        let Some(path) = self.options.screenshot.clone() else { return };
        ctx.request_repaint();
        let elapsed = self.started.elapsed();
        let apps_ready = !self.apps.read().unwrap().is_empty() && self.cfg.pins_seeded;
        let results_ready = self.options.query.is_none() || self.file_micros.is_some();
        let ready = self.frames >= 120
            && elapsed > Duration::from_millis(1200)
            && apps_ready
            && results_ready
            && self.settings.is_settled()
            && self.icons.is_idle();
        if !self.shot_requested && (ready || elapsed > Duration::from_secs(25)) {
            self.shot_requested = true;
            ctx.request_screenshot(move |image| {
                let _ = save_png(&path, &image);
                std::process::exit(0);
            });
        }
    }
}

impl Icon {
    /// Pins are drawn every frame, so don't touch the disk to tell folders from files.
    pub fn for_target_cached(target: &Target) -> Icon {
        match target {
            Target::Path { path, .. } if !path.contains("://") => {
                let looks_like_file = path.rsplit('\\').next().is_some_and(|n| n.contains('.'));
                Icon::Image { key: icons::file_icon_key(path, !looks_like_file), source: path.clone() }
            }
            other => Icon::for_target(other),
        }
    }
}

impl Row {
    /// Its place among the things the keys move between.
    pub(crate) fn nav_id(&self) -> Id {
        Id::unique(("row", &self.name, &self.kind_key()))
    }

    pub(crate) fn kind_key(&self) -> String {
        match &self.kind {
            RowKind::Open(target) => target.key(),
            RowKind::Run(what) => format!("{what:?}"),
            RowKind::Quick(id, _) => format!("{id:?}"),
            RowKind::Web(url) => url.clone(),
            RowKind::Person(person) => format!("person:{}", person.upn),
            RowKind::Link(url) => url.clone(),
            RowKind::Copy(text) => format!("copy:{text}"),
            RowKind::Clip(text) => format!("clip:{}", text.chars().take(200).collect::<String>()),
            RowKind::Output(id) => format!("output:{id}"),
        }
    }
}

impl eframe::App for Menu {
    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // A change from the tray menu lands here.
        self.cfg.take_over_start = self.shared.take_start.load(Ordering::Relaxed);
        self.cfg.take_over_search = self.shared.take_search.load(Ordering::Relaxed);
        self.keep_to_policies();
        crate::i18n::set(crate::i18n::Lang::from_setting(&self.cfg.language));

        for command in self.shared.take_commands() {
            match command {
                Command::Open(_) => self.opened(),
                Command::Customize => self.open_settings(Tab::Appearance),
            }
        }
        self.icons.upload(ctx);
        self.seed_pins();

        // Newly found apps should appear in results already on screen.
        let apps_now = self.apps.read().map(|a| a.len()).unwrap_or(0);
        if apps_now != self.apps_seen {
            self.apps_seen = apps_now;
            if !self.query.trim().is_empty() {
                self.ask_providers(Some(Category::Apps));
            }
        }
        self.keep_m365(ctx);
        self.poll_providers();
        let picked = self.picked.lock().unwrap().take();
        match picked {
            Some(Picked::SearchFolder(path)) => self.add_search_place(path),
            Some(Picked::Pin(path)) => {
                let pin = pins_page::path_pin(&path);
                if let Some(group) = self.cfg.groups.first_mut() {
                    group.pins.push(pin);
                }
            }
            Some(Picked::Bookmark(path)) => {
                if let Some(draft) = self.bookmark_ui.draft.as_mut() {
                    draft.target = path;
                }
            }
            None => {}
        }
        // Windows switched between light and dark mode, or its accent color: the looks that follow it follow at once.
        if system::take_look_changed() {
            self.system_look = SystemLook::read();
            self.mica.desktop_changed();
        }
        // The helpers for bookmarks and what's playing only run while their settings are on.
        let shot = self.is_shot();
        self.media.set_on(self.cfg.media_controls && !shot);
        self.providers.bookmarks.on.store(self.cfg.bookmarks_search && self.cfg.bookmarks_browsers && !shot, Ordering::Relaxed);
        if self.cfg.bookmarks_search {
            self.providers.bookmarks.keep_own(&self.cfg, &crate::bookmarks::bookmarks_of(self.policy.added("/bookmarks")));
        }
        // Folder choices take effect by themselves, a moment after the last change.
        if self.index_dirty_at.is_some_and(|at| at.elapsed() > Duration::from_millis(1200)) {
            self.index_dirty_at = None;
            if !self.is_shot() {
                self.sync_index();
            }
        }
        if self.index_dirty_at.is_some() {
            ctx.request_repaint_after(Duration::from_millis(1300));
        }

        self.handle_keys(ctx);
        self.apply_config_side_effects(ctx);
        self.drive_update(ctx);

        if self.changed_at.is_some_and(|at| at.elapsed() > Duration::from_millis(800)) && !self.is_shot() {
            self.save_settings();
            self.changed_at = None;
        }
        if self.changed_at.is_some() {
            ctx.request_repaint_after(Duration::from_millis(900));
        }
        if self.confirm.is_some() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        self.screenshot_tick(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.frames += 1;
        self.nav_next.clear();
        self.draw(ui);
        std::mem::swap(&mut self.nav, &mut self.nav_next);
        let pal = self.pal();
        let ctx = ui.ctx().clone();
        self.draw_popup(&ctx, pal);
        if self.settings_open {
            crate::customize::show_window(self, &ctx);
        }

        // Results just came in and nothing is picked: the best one gets Enter.
        if !self.query.trim().is_empty() {
            let still_there = self.selected.is_some_and(|id| self.nav.iter().any(|n| n.id == id));
            if !still_there {
                self.selected = self.nav.first().map(|n| n.id);
            }
        }
        for intent in std::mem::take(&mut self.intents) {
            self.apply(intent);
        }
        if self.hide_after_frame && self.frames >= 2 {
            self.hide_after_frame = false;
            self.shared.hide_now();
        }
    }

    fn clear_color(&self, _visuals: &Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
}

/// A test file only counts when taking a picture, so it can never get around a real policy.
fn read_policies(options: &Options) -> Policies {
    match (&options.test_policies, &options.screenshot) {
        (Some(file), Some(_)) => Policies::from_test_file(file),
        _ => Policies::read(),
    }
}

fn window_handle(cc: &eframe::CreationContext<'_>) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match cc.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
        _ => None,
    }
}

