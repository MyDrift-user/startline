//! Bookmarks in search: the person's own list, one an administrator adds, and what's bookmarked in the browsers on
//! this PC. Browser files are only ever read: Chromium browsers' Bookmarks file in every profile, and a copy of
//! Firefox's places.sqlite (with its -wal), which is locked while Firefox runs. They're read again when they change.
//! A bookmark opens in the browser and profile it came from.

use crate::config::{Category, Config, Target};
use crate::i18n::{tr, tx};
use crate::matching;
use crate::providers::{Ask, Provider};
use crate::search::{Icon, Row, RowKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};

/// One of Startline's own bookmarks: a web address, a file, a folder or a command.
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct Bookmark {
    /// Folders inside folders are written with a slash: "Work/Tools". Empty is the top.
    pub folder: String,
    pub name: String,
    pub target: Target,
}

/// The policy's lines: "Folder|Name|Target" or "Folder|Name|Target|Arguments". Lines that aren't either are skipped.
pub fn bookmarks_of(items: &[String]) -> Vec<Bookmark> {
    items
        .iter()
        .filter_map(|item| {
            let parts: Vec<&str> = item.split('|').map(str::trim).collect();
            let (folder, name, target, args) = match parts.as_slice() {
                [folder, name, target] => (*folder, *name, *target, ""),
                [folder, name, target, args] => (*folder, *name, *target, *args),
                _ => return None,
            };
            if name.is_empty() || target.is_empty() {
                return None;
            }
            let target = Target::Path { path: crate::policy::expand(target), args: crate::policy::expand(args) };
            Some(Bookmark { folder: folder.trim_matches('/').to_string(), name: name.to_string(), target })
        })
        .collect()
}

// ---- Browsers ----

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Engine {
    Chromium,
    Firefox,
}

/// A browser profile with bookmarks.
#[derive(Clone, PartialEq, Debug)]
pub struct Profile {
    /// How the settings name it, to leave it out: "Chrome/Default".
    pub key: String,
    pub browser: String,
    /// The name the browser shows for the profile.
    pub name: String,
    pub engine: Engine,
    /// The profile's folder.
    pub dir: PathBuf,
    /// What the browser calls the profile on its command line: the folder name, or Firefox's profile name. Empty when
    /// it has only the one.
    pub arg: String,
    pub program: Option<String>,
}

impl Profile {
    /// "Chrome (Work)", or just "Opera" for a browser without profiles.
    pub fn label(&self) -> String {
        if self.name.is_empty() { self.browser.clone() } else { format!("{} ({})", self.browser, self.name) }
    }

    /// What opens `url` in this browser and profile. Without its program, the default browser opens it.
    pub fn target(&self, url: &str) -> Target {
        let Some(program) = &self.program else { return Target::path(url) };
        // A quote can't end the address early and add switches of its own to the browser's command line.
        let url = url.replace('"', "%22");
        let args = match (self.engine, self.arg.is_empty()) {
            (Engine::Chromium, false) => format!("--profile-directory=\"{}\" \"{url}\"", self.arg),
            (Engine::Firefox, false) => format!("-P \"{}\" -new-tab \"{url}\"", self.arg),
            (Engine::Firefox, true) => format!("-new-tab \"{url}\""),
            (Engine::Chromium, true) => format!("\"{url}\""),
        };
        Target::Path { path: program.clone(), args }
    }

    fn files(&self) -> Vec<PathBuf> {
        match self.engine {
            Engine::Chromium => vec![self.dir.join("Bookmarks")], // i18n-ignore: names on disk, in files and on the command line
            Engine::Firefox => vec![self.dir.join("places.sqlite"), self.dir.join("places.sqlite-wal")],
        }
    }
}

/// A bookmark read from a browser.
#[derive(Clone, PartialEq, Debug)]
pub struct Found {
    pub folder: String,
    pub name: String,
    pub url: String,
}

/// Everything read from the browsers.
#[derive(Default)]
pub struct Library {
    pub profiles: Vec<Profile>,
    /// Each with the index of its profile.
    pub marks: Vec<(usize, Found)>,
}

/// Whether a browser bookmark can be opened from here: not a script and not one of Firefox's saved searches.
fn openable(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    !url.is_empty() && !lower.starts_with("javascript:") && !lower.starts_with("place:") && !lower.starts_with("data:")
}

/// The bookmarks in a Chromium browser's Bookmarks file.
pub fn parse_chromium(json: &str) -> Vec<Found> {
    fn walk(node: &Value, folder: &str, out: &mut Vec<Found>) {
        let name = node.get("name").and_then(Value::as_str).unwrap_or_default();
        match node.get("type").and_then(Value::as_str) {
            Some("url") => {
                let url = node.get("url").and_then(Value::as_str).unwrap_or_default();
                if openable(url) {
                    out.push(Found { folder: folder.to_string(), name: if name.is_empty() { url.to_string() } else { name.to_string() }, url: url.to_string() });
                }
            }
            Some("folder") => {
                let inner = if folder.is_empty() { name.to_string() } else { format!("{folder}/{name}") };
                for child in node.get("children").and_then(Value::as_array).into_iter().flatten() {
                    walk(child, &inner, out);
                }
            }
            _ => {}
        }
    }
    let Ok(file) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    let mut out = Vec::new();
    let roots = [("bookmark_bar", tx("Bookmarks bar")), ("other", tx("Other bookmarks")), ("synced", tx("Mobile bookmarks"))];
    for (key, title) in roots {
        let Some(root) = file.pointer(&format!("/roots/{key}")) else { continue };
        for child in root.get("children").and_then(Value::as_array).into_iter().flatten() {
            walk(child, tr(title), &mut out);
        }
    }
    out
}

/// The bookmarks in a copy of Firefox's places.sqlite. Tags aren't bookmarks of their own, and are left out.
pub fn read_firefox(db: &crate::sqlite::Db) -> Vec<Found> {
    let sql = "SELECT b.id, b.parent, b.type, b.title, p.url, b.guid FROM moz_bookmarks b LEFT JOIN moz_places p ON p.id = b.fk ORDER BY b.parent, b.position"; // i18n-ignore: names on disk, in files and on the command line
    let Some(rows) = db.rows(sql) else { return Vec::new() };
    struct Item {
        parent: i64,
        kind: i64,
        title: String,
        url: String,
        guid: String,
    }
    let number = |text: &Option<String>| text.as_deref().and_then(|t| t.parse::<i64>().ok()).unwrap_or(0);
    let text = |text: &Option<String>| text.clone().unwrap_or_default();
    let items: std::collections::HashMap<i64, Item> = rows
        .iter()
        .filter(|row| row.len() == 6)
        .map(|row| (number(&row[0]), Item { parent: number(&row[1]), kind: number(&row[2]), title: text(&row[3]), url: text(&row[4]), guid: text(&row[5]) }))
        .collect();
    let root_title = |guid: &str| match guid {
        "menu________" => Some(tr(tx("Bookmarks menu"))),
        "toolbar_____" => Some(tr(tx("Bookmarks bar"))),
        "unfiled_____" => Some(tr(tx("Other bookmarks"))),
        "mobile______" => Some(tr(tx("Mobile bookmarks"))),
        _ => None,
    };
    let mut out = Vec::new();
    let mut ids: Vec<&i64> = items.keys().collect();
    ids.sort();
    for id in ids {
        let item = &items[id];
        if item.kind != 1 || !openable(&item.url) {
            continue;
        }
        // Up through the folders to the top, which isn't shown.
        let mut path: Vec<String> = Vec::new();
        let mut at = item.parent;
        let mut tagged = false;
        for _ in 0..64 {
            let Some(folder) = items.get(&at) else { break };
            if folder.guid == "root________" {
                break;
            }
            if folder.guid == "tags________" {
                tagged = true;
                break;
            }
            path.push(root_title(&folder.guid).map_or_else(|| folder.title.clone(), str::to_string));
            at = folder.parent;
        }
        if tagged {
            continue;
        }
        path.reverse();
        let name = if item.title.is_empty() { item.url.clone() } else { item.title.clone() };
        out.push(Found { folder: path.join("/"), name, url: item.url.clone() });
    }
    out
}

/// The profiles in a Firefox profiles.ini: each one's name and folder.
pub fn parse_profiles_ini(text: &str, base: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let (mut name, mut path, mut relative, mut in_profile) = (String::new(), String::new(), true, false);
    let mut finish = |name: &mut String, path: &mut String, relative: bool, in_profile: bool| {
        if in_profile && !path.is_empty() {
            let dir = if relative { base.join(path.replace('/', "\\")) } else { PathBuf::from(path.as_str()) };
            out.push((std::mem::take(name), dir));
        }
        name.clear();
        path.clear();
    };
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            finish(&mut name, &mut path, relative, in_profile);
            in_profile = line.to_ascii_lowercase().starts_with("[profile");
            relative = true;
        } else if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "Name" => name = value.trim().to_string(), // i18n-ignore: names on disk, in files and on the command line
                "Path" => path = value.trim().to_string(), // i18n-ignore: names on disk, in files and on the command line
                "IsRelative" => relative = value.trim() != "0",
                _ => {}
            }
        }
    }
    finish(&mut name, &mut path, relative, in_profile);
    out
}

/// A name for a Chromium browser from where it keeps its data, relative to the AppData folder it's in.
pub fn chromium_name(relative: &Path) -> String {
    let parts: Vec<String> = relative.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).filter(|p| !p.eq_ignore_ascii_case("User Data")).collect(); // i18n-ignore: names on disk, in files and on the command line
    let joined = parts.join("\\").to_ascii_lowercase();
    let known = [
        ("google\\chrome beta", "Chrome Beta"), // i18n-ignore: browser names
        ("google\\chrome sxs", "Chrome Canary"), // i18n-ignore: browser names
        ("google\\chrome", "Chrome"), // i18n-ignore: browser names
        ("microsoft\\edge beta", "Edge Beta"), // i18n-ignore: browser names
        ("microsoft\\edge", "Edge"), // i18n-ignore: browser names
        ("bravesoftware\\brave-browser", "Brave"), // i18n-ignore: browser names
        ("opera software\\opera gx stable", "Opera GX"), // i18n-ignore: browser names
        ("opera software\\opera stable", "Opera"), // i18n-ignore: browser names
        ("imput\\helium", "Helium"), // i18n-ignore: browser names
    ];
    if let Some((_, name)) = known.iter().find(|(path, _)| joined == *path) {
        return name.to_string();
    }
    let last = parts.last().cloned().unwrap_or_default();
    last.trim_end_matches(" Stable").to_string() // i18n-ignore: names on disk, in files and on the command line
}

/// Every Chromium profile with a Bookmarks file, in browsers that keep their data in AppData.
fn chromium_profiles() -> Vec<Profile> {
    let mut out = Vec::new();
    for base in ["LOCALAPPDATA", "APPDATA"].iter().filter_map(std::env::var_os).map(PathBuf::from) {
        let mut data_dirs = Vec::new();
        for first in read_dirs(&base) {
            data_dirs.push(first.join("User Data")); // i18n-ignore: names on disk, in files and on the command line
            data_dirs.push(first.clone());
            for second in read_dirs(&first) {
                data_dirs.push(second.join("User Data")); // i18n-ignore: names on disk, in files and on the command line
                data_dirs.push(second);
            }
        }
        for data in data_dirs.into_iter().filter(|d| d.join("Local State").is_file()) { // i18n-ignore: names on disk, in files and on the command line
            let relative = data.strip_prefix(&base).unwrap_or(&data).to_path_buf();
            let browser = chromium_name(&relative);
            let names = profile_names(&data.join("Local State")); // i18n-ignore: names on disk, in files and on the command line
            let program = chromium_program(&base, &data, &browser);
            // Opera keeps its one profile right in its data folder.
            let mut dirs: Vec<PathBuf> = vec![data.clone()];
            dirs.extend(read_dirs(&data));
            for dir in dirs.into_iter().filter(|d| d.join("Bookmarks").is_file()) { // i18n-ignore: names on disk, in files and on the command line
                let folder = if dir == data { String::new() } else { dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() };
                let name = names.iter().find(|(f, _)| *f == folder).map_or_else(|| folder.clone(), |(_, n)| n.clone());
                let key = format!("{browser}/{}", if folder.is_empty() { "-" } else { &folder });
                if out.iter().any(|p: &Profile| p.key == key) {
                    continue;
                }
                out.push(Profile { key, browser: browser.clone(), name, engine: Engine::Chromium, dir, arg: folder, program: program.clone() });
            }
        }
    }
    // Only one profile: no need to name it.
    let counts = |browser: &str| out.iter().filter(|p| p.browser == browser).count();
    let single: Vec<bool> = out.iter().map(|p| counts(&p.browser) == 1).collect();
    for (profile, alone) in out.iter_mut().zip(single) {
        if alone {
            profile.name.clear();
        }
    }
    out
}

/// What each profile folder is called in the browser, from its Local State file.
fn profile_names(local_state: &Path) -> Vec<(String, String)> {
    let Some(state) = std::fs::read(local_state).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { return Vec::new() };
    let Some(cache) = state.pointer("/profile/info_cache").and_then(Value::as_object) else { return Vec::new() };
    cache.iter().map(|(dir, info)| (dir.clone(), info.get("name").and_then(Value::as_str).unwrap_or(dir).to_string())).collect()
}

fn read_dirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect()
}

const CHROMIUM_PROGRAMS: [&str; 7] = ["chrome.exe", "msedge.exe", "brave.exe", "vivaldi.exe", "helium.exe", "chromium.exe", "opera.exe"];

/// The browser's program: beside its data, under Program Files, or where Windows was told it is.
fn chromium_program(base: &Path, data: &Path, browser: &str) -> Option<String> {
    let home = if data.ends_with("User Data") { data.parent()? } else { data }; // i18n-ignore: names on disk, in files and on the command line
    let relative = home.strip_prefix(base).ok()?.to_path_buf();
    let mut places = vec![home.join("Application")]; // i18n-ignore: names on disk, in files and on the command line
    for root in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"].iter().filter_map(std::env::var_os).map(PathBuf::from) {
        places.push(root.join(&relative).join("Application")); // i18n-ignore: names on disk, in files and on the command line
    }
    if browser.starts_with("Opera") { // i18n-ignore: names on disk, in files and on the command line
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let folder = PathBuf::from(local).join("Programs").join(browser); // i18n-ignore: names on disk, in files and on the command line
            for program in ["launcher.exe", "opera.exe"] {
                if folder.join(program).is_file() {
                    return Some(folder.join(program).to_string_lossy().into_owned());
                }
            }
        }
    }
    for place in &places {
        for program in CHROMIUM_PROGRAMS {
            let path = place.join(program);
            if path.is_file() {
                return Some(path.to_string_lossy().into_owned());
            }
        }
    }
    let guess = format!("{}.exe", browser.to_ascii_lowercase().replace(' ', ""));
    registered_program(&guess)
}

/// Where Windows was told a program is (App Paths).
fn registered_program(exe: &str) -> Option<String> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let key = format!("Software\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{exe}");
    [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
        .into_iter()
        .filter_map(|root| crate::platform::reg_text(root, &key, ""))
        .map(|path| path.trim_matches('"').to_string())
        .find(|path| Path::new(path).is_file())
}

/// Firefox and the browsers built like it, each with a profiles.ini in AppData.
fn firefox_profiles() -> Vec<Profile> {
    let mut out = Vec::new();
    let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) else { return out };
    let mut inis = Vec::new();
    for first in read_dirs(&appdata) {
        inis.push(first.join("profiles.ini"));
        for second in read_dirs(&first) {
            inis.push(second.join("profiles.ini"));
        }
    }
    for ini in inis.into_iter().filter(|i| i.is_file()) {
        let Some(base) = ini.parent() else { continue };
        let Ok(text) = std::fs::read_to_string(&ini) else { continue };
        let folder = base.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let browser = match folder.to_ascii_lowercase().as_str() {
            "firefox" => "Firefox".to_string(), // i18n-ignore: browser name
            _ => folder.clone(),
        };
        let exe = format!("{}.exe", browser.to_ascii_lowercase().replace(' ', ""));
        let program = registered_program(&exe).or_else(|| {
            ["ProgramFiles", "ProgramFiles(x86)"]
                .iter()
                .filter_map(std::env::var_os)
                .map(|root| PathBuf::from(root).join(if browser == "Firefox" { "Mozilla Firefox" } else { &browser }).join(&exe)) // i18n-ignore: names on disk, in files and on the command line
                .find(|p| p.is_file())
                .map(|p| p.to_string_lossy().into_owned())
        });
        let profiles: Vec<(String, PathBuf)> = parse_profiles_ini(&text, base).into_iter().filter(|(_, dir)| dir.join("places.sqlite").is_file()).collect();
        let alone = profiles.len() == 1;
        for (name, dir) in profiles {
            let key = format!("{browser}/{name}");
            if out.iter().any(|p: &Profile| p.key == key) {
                continue;
            }
            let shown = if alone { String::new() } else { name.clone() };
            out.push(Profile { key, browser: browser.clone(), name: shown, engine: Engine::Firefox, dir, arg: if alone { String::new() } else { name }, program: program.clone() });
        }
    }
    out
}

/// The biggest Bookmarks file read. Firefox's places, which holds the history too, may be four times as big.
const MOST_BYTES: u64 = 64 * 1024 * 1024;

/// Reads one profile's bookmarks. Firefox's are read from a copy in the temp folder, deleted after.
fn read_profile(profile: &Profile) -> Vec<Found> {
    match profile.engine {
        Engine::Chromium => {
            let file = profile.dir.join("Bookmarks"); // i18n-ignore: names on disk, in files and on the command line
            // A file far bigger than any real bookmarks isn't read.
            if std::fs::metadata(&file).map_or(true, |m| m.len() > MOST_BYTES) {
                return Vec::new();
            }
            std::fs::read_to_string(file).map(|text| parse_chromium(&text)).unwrap_or_default()
        }
        Engine::Firefox => {
            let copy = std::env::temp_dir().join(format!("startline-places-{}.sqlite", std::process::id())); // i18n-ignore: file name
            let wal = PathBuf::from(format!("{}-wal", copy.display()));
            let shm = PathBuf::from(format!("{}-shm", copy.display()));
            let _ = std::fs::remove_file(&wal);
            let _ = std::fs::remove_file(&shm);
            let places = profile.dir.join("places.sqlite");
            if std::fs::metadata(&places).map_or(true, |m| m.len() > 4 * MOST_BYTES) || std::fs::copy(&places, &copy).is_err() {
                let _ = std::fs::remove_file(&copy);
                return Vec::new();
            }
            let _ = std::fs::copy(profile.dir.join("places.sqlite-wal"), &wal);
            let found = crate::sqlite::Db::open(&copy, false).map(|db| read_firefox(&db)).unwrap_or_default();
            for file in [&copy, &wal, &shm] {
                let _ = std::fs::remove_file(file);
            }
            found
        }
    }
}

/// When and how big each file is: a change means reading again.
fn stamp(profiles: &[Profile]) -> Vec<(PathBuf, Option<SystemTime>, u64)> {
    profiles
        .iter()
        .flat_map(Profile::files)
        .map(|file| {
            let meta = std::fs::metadata(&file).ok();
            let when = meta.as_ref().and_then(|m| m.modified().ok());
            (file, when, meta.map_or(0, |m| m.len()))
        })
        .collect()
}

/// Keeps the browsers' bookmarks read while `on`: looks for profiles now and then, and reads them again when their
/// files change.
pub fn watch(ctx: egui::Context, on: Arc<AtomicBool>) -> (Arc<RwLock<Library>>, Arc<AtomicU64>) {
    let library: Arc<RwLock<Library>> = Arc::default();
    let generation = Arc::new(AtomicU64::new(0));
    let (shared, changed) = (library.clone(), generation.clone());
    let _ = std::thread::Builder::new().name("bookmarks".into()).spawn(move || {
        let mut profiles: Vec<Profile> = Vec::new();
        let mut looked: Option<Instant> = None;
        let mut last = Vec::new();
        let mut read_at: Option<Instant> = None;
        loop {
            std::thread::sleep(Duration::from_secs(if last.is_empty() { 1 } else { 4 }));
            if !on.load(Ordering::Relaxed) {
                if !profiles.is_empty() {
                    profiles.clear();
                    last.clear();
                    looked = None;
                    read_at = None;
                    *shared.write().unwrap() = Library::default();
                }
                continue;
            }
            if looked.is_none_or(|at| at.elapsed() > Duration::from_secs(60)) {
                looked = Some(Instant::now());
                profiles = chromium_profiles();
                profiles.extend(firefox_profiles());
            }
            let now = stamp(&profiles);
            if now == last {
                continue;
            }
            // Firefox's places changes with every page visited: it's copied and read at most twice a minute.
            if read_at.is_some_and(|at| at.elapsed() < Duration::from_secs(30)) {
                continue;
            }
            read_at = Some(Instant::now());
            last = now;
            let marks = profiles.iter().enumerate().flat_map(|(at, profile)| read_profile(profile).into_iter().map(move |found| (at, found))).collect();
            *shared.write().unwrap() = Library { profiles: profiles.clone(), marks };
            changed.fetch_add(1, Ordering::Relaxed);
            ctx.request_repaint();
        }
    });
    (library, generation)
}

/// The browser profiles found on this PC and how many bookmarks each has, for checking on a real machine. Only reads.
pub fn probe_report() -> String {
    let mut profiles = chromium_profiles();
    profiles.extend(firefox_profiles());
    let mut out = format!("bookmark profiles={}\n", profiles.len()); // i18n-ignore: probe report for tests
    for profile in &profiles {
        let started = Instant::now();
        let count = read_profile(profile).len();
        out.push_str(&format!("  {} bookmarks={count} program={} read in {:?}\n", profile.key, profile.program.is_some(), started.elapsed())); // i18n-ignore: probe report for tests
    }
    out
}

// ---- In search ----

/// The bookmarks in search.
pub struct Bookmarks {
    pub library: Arc<RwLock<Library>>,
    generation: Arc<AtomicU64>,
    seen: u64,
    /// Keeps the browser reader going.
    pub on: Arc<AtomicBool>,
    /// What the last search needs to be worked out again when the browsers' bookmarks change.
    last: Option<(Vec<String>, Config, Vec<Bookmark>)>,
    rows: Vec<Row>,
}

impl Bookmarks {
    pub fn start(ctx: egui::Context) -> Bookmarks {
        let on = Arc::new(AtomicBool::new(false));
        let (library, generation) = watch(ctx, on.clone());
        Bookmarks { library, generation, seen: 0, on, last: None, rows: Vec::new() }
    }

    fn work_out(&mut self) {
        let Some((tokens, cfg, managed)) = &self.last else { return };
        self.rows = rows(tokens, cfg, managed, &self.library.read().unwrap());
    }
}

/// Bookmarks searched in by the person: their own, the administrator's, then the browsers' they haven't left out.
pub fn rows(tokens: &[String], cfg: &Config, managed: &[Bookmark], library: &Library) -> Vec<Row> {
    const MOST: usize = 8;
    let mut scored: Vec<(i32, Row)> = Vec::new();
    let own = |mark: &Bookmark, theirs: bool| {
        let where_ = target_text(&mark.target);
        let score = matching::score_all(&matching::lower(&mark.name), &matching::lower(&format!("{where_} {}", mark.folder)), tokens)?;
        let detail = if mark.folder.is_empty() { tr("Bookmark").to_string() } else { crate::trf!("Bookmark in {folder}", folder = mark.folder) };
        let detail = if theirs { detail } else { crate::trf!("{detail}, from your organization", detail = detail) };
        let icon = match &mark.target {
            Target::Path { path, .. } if path.contains("://") => Icon::Glyph('\u{E734}'),
            other => Icon::for_target(other),
        };
        Some((score + cfg.usage_boost(&mark.target.key()), Row { category: Category::Bookmarks, name: mark.name.clone(), detail, icon, kind: RowKind::Open(mark.target.clone()), path: None }))
    };
    scored.extend(cfg.bookmarks.iter().filter_map(|m| own(m, true)));
    scored.extend(managed.iter().filter_map(|m| own(m, false)));
    if cfg.bookmarks_browsers {
        for (at, found) in &library.marks {
            let Some(profile) = library.profiles.get(*at) else { continue };
            if cfg.bookmarks_left_out.iter().any(|key| key == &profile.key) {
                continue;
            }
            let Some(score) = matching::score_all(&matching::lower(&found.name), &matching::lower(&format!("{} {}", found.url, found.folder)), tokens) else { continue };
            let target = profile.target(&found.url);
            let folder = if found.folder.is_empty() { String::new() } else { format!("{}, ", found.folder) };
            let row = Row {
                category: Category::Bookmarks,
                name: found.name.clone(),
                detail: format!("{folder}{}", profile.label()),
                icon: Icon::Glyph('\u{E728}'),
                kind: RowKind::Open(target.clone()),
                path: None,
            };
            scored.push((score + cfg.usage_boost(&target.key()), row));
        }
    }
    let any_real = scored.iter().any(|(score, _)| *score > matching::LOOSE);
    scored.retain(|(score, _)| !any_real || *score > matching::LOOSE);
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    let mut out: Vec<Row> = Vec::new();
    for (_, row) in scored {
        // The same address bookmarked twice in one place shows once.
        if !out.iter().any(|r| r.kind == row.kind && r.name == row.name) {
            out.push(row);
        }
        if out.len() == MOST {
            break;
        }
    }
    out
}

/// A target's address, path or command as one line, to search in and show.
pub fn target_text(target: &Target) -> String {
    match target {
        Target::App(id) => id.clone(),
        Target::Path { path, args } if args.is_empty() => path.clone(),
        Target::Path { path, args } => format!("{path} {args}"),
    }
}

impl Provider for Bookmarks {
    fn category(&self) -> Category {
        Category::Bookmarks
    }

    fn ask(&mut self, ask: &Ask) {
        if !ask.cfg.bookmarks_search || !ask.cfg.shows(Category::Bookmarks) || ask.tokens.is_empty() {
            self.last = None;
            self.rows.clear();
            return;
        }
        let managed = bookmarks_of(ask.policy.added("/bookmarks"));
        self.last = Some((ask.tokens.to_vec(), ask.cfg.clone(), managed));
        self.work_out();
    }

    fn clear(&mut self) {
        self.last = None;
        self.rows.clear();
    }

    fn poll(&mut self) -> bool {
        let now = self.generation.load(Ordering::Relaxed);
        if now == self.seen {
            return false;
        }
        self.seen = now;
        if self.last.is_none() {
            return false;
        }
        self.work_out();
        true
    }

    fn rows(&self, _: &Config) -> Vec<Row> {
        self.rows.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHROMIUM: &str = r#"{
        "checksum": "x", "version": 1,
        "roots": {
            "bookmark_bar": {"type": "folder", "name": "Bookmarks bar", "children": [
                {"type": "url", "name": "Rust", "url": "https://www.rust-lang.org/"},
                {"type": "folder", "name": "Work", "children": [
                    {"type": "url", "name": "Tickets", "url": "https://tickets.example.com/"},
                    {"type": "folder", "name": "Old", "children": [{"type": "url", "name": "", "url": "https://old.example.com/"}]}
                ]},
                {"type": "url", "name": "Dark mode", "url": "javascript:void(document.body.style.background='black')"}
            ]},
            "other": {"type": "folder", "name": "Other bookmarks", "children": [{"type": "url", "name": "Recipes", "url": "https://food.example.com/"}]},
            "synced": {"type": "folder", "name": "Mobile bookmarks", "children": []}
        }
    }"#;

    #[test]
    fn chromium_bookmarks_are_read_with_their_folders() {
        let found = parse_chromium(CHROMIUM);
        let got: Vec<(&str, &str, &str)> = found.iter().map(|f| (f.folder.as_str(), f.name.as_str(), f.url.as_str())).collect();
        assert_eq!(
            got,
            [
                ("Bookmarks bar", "Rust", "https://www.rust-lang.org/"),
                ("Bookmarks bar/Work", "Tickets", "https://tickets.example.com/"),
                ("Bookmarks bar/Work/Old", "https://old.example.com/", "https://old.example.com/"),
                ("Other bookmarks", "Recipes", "https://food.example.com/"),
            ],
            "a script bookmark can't be opened from here, and one without a name goes by its address"
        );
        assert!(parse_chromium("not json").is_empty());
        assert!(parse_chromium("{}").is_empty());
    }

    #[test]
    fn firefox_bookmarks_are_read_from_a_copy_of_places() {
        if !crate::sqlite::available() {
            return;
        }
        let path = std::env::temp_dir().join(format!("startline-test-places-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let db = crate::sqlite::Db::open(&path, true).expect("a database in the temp folder");
            // The parts of Firefox's own schema that are read, with its fixed root folders.
            assert!(db.exec(
                "CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url LONGVARCHAR, title LONGVARCHAR, rev_host LONGVARCHAR, visit_count INTEGER DEFAULT 0, guid TEXT);
                 CREATE TABLE moz_bookmarks (id INTEGER PRIMARY KEY, type INTEGER, fk INTEGER DEFAULT NULL, parent INTEGER, position INTEGER, title LONGVARCHAR, keyword_id INTEGER, folder_type TEXT, dateAdded INTEGER, lastModified INTEGER, guid TEXT, syncStatus INTEGER NOT NULL DEFAULT 0, syncChangeCounter INTEGER NOT NULL DEFAULT 1);
                 INSERT INTO moz_places (id, url, title) VALUES (1, 'https://www.mozilla.org/', 'Mozilla'), (2, 'https://docs.example.com/', 'Docs'), (3, 'place:sort=8', NULL), (4, 'https://tagged.example.com/', 'Tagged');
                 INSERT INTO moz_bookmarks (id, type, fk, parent, position, title, guid) VALUES
                   (1, 2, NULL, 0, 0, '', 'root________'),
                   (2, 2, NULL, 1, 0, 'menu', 'menu________'),
                   (3, 2, NULL, 1, 1, 'toolbar', 'toolbar_____'),
                   (4, 2, NULL, 1, 2, 'tags', 'tags________'),
                   (5, 2, NULL, 1, 3, 'unfiled', 'unfiled_____'),
                   (6, 2, NULL, 1, 4, 'mobile', 'mobile______'),
                   (10, 1, 1, 3, 0, 'Mozilla', 'a1'),
                   (11, 2, NULL, 2, 0, 'Reading', 'f1'),
                   (12, 1, 2, 11, 0, 'Docs', 'a2'),
                   (13, 1, 3, 5, 0, 'Most Visited', 'a3'),
                   (14, 2, NULL, 4, 0, 'work', 'tag1'),
                   (15, 1, 4, 14, 0, NULL, 'a4');"
            ));
            let found = read_firefox(&db);
            let got: Vec<(&str, &str, &str)> = found.iter().map(|f| (f.folder.as_str(), f.name.as_str(), f.url.as_str())).collect();
            assert_eq!(
                got,
                [("Bookmarks bar", "Mozilla", "https://www.mozilla.org/"), ("Bookmarks menu/Reading", "Docs", "https://docs.example.com/")],
                "saved searches and tags aren't bookmarks"
            );
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn firefox_profiles_are_found_in_profiles_ini() {
        let ini = "[Install4F96D1932A9F858E]\nDefault=Profiles/abc.default-release\n\n[Profile1]\nName=default\nIsRelative=1\nPath=Profiles/xyz.default\n\n[Profile0]\nName=Work\nIsRelative=0\nPath=D:\\Firefox\\work\nDefault=1\n\n[General]\nStartWithLastProfile=1\n";
        let base = Path::new("C:\\Users\\me\\AppData\\Roaming\\Mozilla\\Firefox");
        let found = parse_profiles_ini(ini, base);
        assert_eq!(found, [("default".to_string(), base.join("Profiles\\xyz.default")), ("Work".to_string(), PathBuf::from("D:\\Firefox\\work"))]);
    }

    #[test]
    fn browsers_are_named_and_open_bookmarks_in_their_own_profile() {
        assert_eq!(chromium_name(Path::new("Google\\Chrome\\User Data")), "Chrome");
        assert_eq!(chromium_name(Path::new("BraveSoftware\\Brave-Browser\\User Data")), "Brave");
        assert_eq!(chromium_name(Path::new("Opera Software\\Opera Stable")), "Opera");
        assert_eq!(chromium_name(Path::new("Vivaldi\\User Data")), "Vivaldi");
        assert_eq!(chromium_name(Path::new("Thorium\\User Data")), "Thorium", "others go by their folder");
        let chrome = Profile { key: "Chrome/Profile 2".into(), browser: "Chrome".into(), name: "Work".into(), engine: Engine::Chromium, dir: PathBuf::new(), arg: "Profile 2".into(), program: Some("C:\\chrome.exe".into()) };
        assert_eq!(chrome.label(), "Chrome (Work)");
        assert_eq!(chrome.target("https://a.example/"), Target::Path { path: "C:\\chrome.exe".into(), args: "--profile-directory=\"Profile 2\" \"https://a.example/\"".into() });
        let firefox = Profile { engine: Engine::Firefox, arg: "Work".into(), program: Some("C:\\firefox.exe".into()), ..chrome.clone() };
        assert_eq!(firefox.target("https://a.example/"), Target::Path { path: "C:\\firefox.exe".into(), args: "-P \"Work\" -new-tab \"https://a.example/\"".into() });
        assert_eq!(
            chrome.target("https://a.example/\" --evil"),
            Target::Path { path: "C:\\chrome.exe".into(), args: "--profile-directory=\"Profile 2\" \"https://a.example/%22 --evil\"".into() },
            "a quote can't end the address"
        );
        let lost = Profile { program: None, ..chrome };
        assert_eq!(lost.target("https://a.example/"), Target::path("https://a.example/"), "without its program, the default browser opens it");
    }

    #[test]
    fn bookmarks_are_found_by_name_address_or_folder() {
        let mut cfg = Config { roots: vec![], bookmarks_search: true, ..Default::default() };
        cfg.bookmarks = vec![Bookmark { folder: "Work/Tools".into(), name: "Build server".into(), target: Target::path("https://ci.example.com/") }];
        let managed = bookmarks_of(&["Company|Intranet|https://intranet.example.com/".into(), "not a bookmark".into()]);
        assert_eq!(managed.len(), 1);
        let chrome = Profile { key: "Chrome/Default".into(), browser: "Chrome".into(), name: String::new(), engine: Engine::Chromium, dir: PathBuf::new(), arg: "Default".into(), program: None };
        let library = Library { profiles: vec![chrome], marks: vec![(0, Found { folder: "Bookmarks bar".into(), name: "Build logs".into(), url: "https://logs.example.com/".into() })] };
        let names = |query: &str, cfg: &Config| rows(&matching::tokens(query), cfg, &managed, &library).into_iter().map(|r| (r.name, r.detail)).collect::<Vec<_>>();
        let found = names("build", &cfg);
        assert_eq!(found.len(), 2);
        assert!(found.contains(&("Build server".into(), "Bookmark in Work/Tools".into())));
        assert!(found.contains(&("Build logs".into(), "Bookmarks bar, Chrome".into())));
        assert_eq!(names("intranet", &cfg), [("Intranet".to_string(), "Bookmark in Company, from your organization".to_string())]);
        assert_eq!(names("ci.example", &cfg)[0].0, "Build server", "the address finds it too");
        cfg.bookmarks_left_out = vec!["Chrome/Default".into()];
        assert_eq!(names("logs", &cfg), Vec::<(String, String)>::new(), "a profile left out isn't searched");
        cfg.bookmarks_left_out.clear();
        cfg.bookmarks_browsers = false;
        assert!(names("logs", &cfg).is_empty(), "nor any browser with them switched off");
    }
}
