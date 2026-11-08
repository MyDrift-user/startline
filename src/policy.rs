//! Company policies: one list of everything an administrator can set. The Group Policy
//! templates are written from it, the app reads the same names back from the Registry, and a
//! test fails if a setting is ever added without a policy. A policy wins over the person's own
//! choice without replacing it: remove the policy and their choice comes back.

use crate::actions::{ACTIONS, Do};
use crate::config::{Category, Config, PRESETS, Pin, PinGroup, Section, SectionKind, Target};
use crate::customize::Tab;
use crate::platform;
use crate::scope::SKIP_PRESETS;
use crate::search::RowKind;
use crate::settings;
use crate::setting_list::QuickId;
use crate::ui::{Intent, Page};
use egui::Ui;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;
use windows::Win32::System::Registry::*;
use windows::core::{PCWSTR, PWSTR};

const KEY: &str = "Software\\Policies\\Startline";
pub const THEME_KEYS: [&str; 6] = ["ColorTheme", "Color_Background", "Color_Panels", "Color_Text", "Color_QuietText", "Color_Accent"];
const AUTOSTART: &str = "StartWithWindows";
const PINS: &str = "PinnedApps";
const PAGES: &str = "WindowsSettingsPages";
const WINDOWS_SETTINGS: &str = "ChangeWindowsSettings";
const FILE_BUTTONS: &str = "FileTypeButtons";
pub const CHECK_FOR_UPDATES: &str = "CheckForUpdates";

/// Saved in the settings file, but only ever remembered for the person, never chosen by them.
#[cfg(test)]
const NOT_SETTINGS: [&str; 6] = ["version", "pins_seeded", "autostart_applied", "settings_pos", "settings_size", "usage"];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Group {
    Appearance,
    StartMenu,
    Pins,
    Search,
    General,
    Updates,
    Limits,
}

impl Group {
    const ALL: [(Group, &'static str, &'static str); 7] = [
        (Group::Appearance, "Appearance", "Appearance"),
        (Group::StartMenu, "StartMenu", "Start menu"),
        (Group::Pins, "Pins", "Pinned apps"),
        (Group::Search, "Search", "Search"),
        (Group::General, "General", "General"),
        (Group::Updates, "Updates", "Updates"),
        (Group::Limits, "Limits", "What people can do"),
    ];

    fn tab(self) -> Tab {
        match self {
            Group::Appearance => Tab::Appearance,
            Group::StartMenu => Tab::StartMenu,
            Group::Pins => Tab::Pins,
            Group::Search => Tab::Search,
            Group::General | Group::Limits => Tab::General,
            Group::Updates => Tab::About,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Kind {
    Bool,
    Number { min: u32, max: u32, divide: u32, default: u32 },
    Choice(Vec<(String, String)>),
    Text(&'static str),
    Color(&'static str),
    List,
    /// One tick box per item, all ticked to begin with.
    Checks(Vec<(String, String)>),
}

#[derive(Clone, Debug)]
pub struct Policy {
    pub key: String,
    /// Where it lands in the saved settings, if it's one of them.
    pub field: &'static str,
    pub group: Group,
    pub title: String,
    pub about: String,
    pub kind: Kind,
}

fn entry(key: &str, field: &'static str, group: Group, title: &str, about: &str, kind: Kind) -> Policy {
    Policy { key: key.to_string(), field, group, title: title.to_string(), about: about.to_string(), kind }
}

fn switch(key: &str, field: &'static str, group: Group, title: &str, about: &str) -> Policy {
    entry(key, field, group, title, about, Kind::Bool)
}

fn number(key: &str, field: &'static str, group: Group, title: &str, about: &str, min: u32, max: u32, default: u32) -> Policy {
    entry(key, field, group, title, about, Kind::Number { min, max, divide: 1, default })
}

fn options(pairs: &[(&str, &str)]) -> Kind {
    Kind::Choice(pairs.iter().map(|(value, label)| (value.to_string(), label.to_string())).collect())
}

fn list(key: &str, field: &'static str, group: Group, title: &str, about: &str) -> Policy {
    entry(key, field, group, title, about, Kind::List)
}

/// The name of the policy for one of the "What to skip" switches.
pub fn skip_key(title: &str) -> String {
    let word: String = title.chars().filter(char::is_ascii_alphanumeric).collect();
    format!("Skip_{word}")
}

fn command_id(what: Do) -> Option<&'static str> {
    match what {
        Do::Open(_) => None,
        Do::Lock => Some("Command_Lock"),
        Do::Sleep => Some("Command_Sleep"),
        Do::SignOut => Some("Command_SignOut"),
        Do::SwitchUser => Some("Command_SwitchUser"),
        Do::Restart => Some("Command_Restart"),
        Do::ShutDown => Some("Command_ShutDown"),
        Do::EmptyRecycleBin => Some("Command_EmptyRecycleBin"),
        Do::Customize => Some("Command_Customize"),
        Do::PinPage => Some("Command_PinPage"),
        Do::RealStart => Some("Command_RealStart"),
        Do::Quit => Some("Command_Quit"),
    }
}

/// Everything an administrator can set.
pub fn all() -> &'static [Policy] {
    static LIST: OnceLock<Vec<Policy>> = OnceLock::new();
    LIST.get_or_init(build)
}

fn build() -> Vec<Policy> {
    use Group::*;
    const STARTLINE_ONLY: &str = "Only the Startline look uses this.";
    let mut list = vec![
        // Appearance
        entry("Experience", "/experience", Appearance, "Look", "Which look the Start menu, search and the settings window use.", options(&[("Windows11", "Windows 11"), ("Windows10", "Windows 10"), ("Windows7", "Windows 7"), ("WindowsXp", "Windows XP"), ("Startline", "Startline")])),
        entry("TextAndIconSize", "/ui_scale", Appearance, "Text and icon size", "In percent. The settings window offers 90 (small), 100 (normal) and 115 (large).", Kind::Number { min: 80, max: 150, divide: 100, default: 100 }),
        entry("ColorTheme", "/preset", Appearance, "Color theme", STARTLINE_ONLY, Kind::Choice(PRESETS.iter().map(|(name, _)| (name.to_string(), name.to_string())).collect())),
        entry("Color_Background", "/theme/bg", Appearance, "Background color", "Written as #RRGGBB. Replaces this color of the theme. Only the Startline look uses this.", Kind::Color("#1B1B1A")),
        entry("Color_Panels", "/theme/surface", Appearance, "Panel color", "Written as #RRGGBB. Replaces this color of the theme. Only the Startline look uses this.", Kind::Color("#272725")),
        entry("Color_Text", "/theme/text", Appearance, "Text color", "Written as #RRGGBB. Replaces this color of the theme. Only the Startline look uses this.", Kind::Color("#ECEAE5")),
        entry("Color_QuietText", "/theme/subtext", Appearance, "Quiet text color", "Written as #RRGGBB. Replaces this color of the theme. Only the Startline look uses this.", Kind::Color("#94928C")),
        entry("Color_Accent", "/theme/accent", Appearance, "Accent color", "Written as #RRGGBB. Replaces this color of the theme. Only the Startline look uses this.", Kind::Color("#D9A46B")),
        entry("Corners", "/corners", Appearance, "Window corners", STARTLINE_ONLY, options(&[("Round", "Round"), ("Slight", "Slight"), ("Square", "Square")])),
        switch("FrostedGlass", "/blur", Appearance, "Frosted glass", "Blurs what's behind the menu. Only the Startline look uses this."),
        entry("Opacity", "/opacity", Appearance, "How solid the menu is", "In percent, from 50 (most see-through) to 100 (solid). Only the Startline look uses this.", Kind::Number { min: 50, max: 100, divide: 100, default: 96 }),
        // Start menu
        list("Sections", "/sections", StartMenu, "What the Start menu shows", "One per line, in the order they should appear: Pinned, Recent, Frequent, AllApps. Any left out are hidden."),
        number("RecentCount", "/recent_count", StartMenu, "Number of recent files", "", 2, 20, 6),
        number("FrequentCount", "/frequent_count", StartMenu, "Number of most-used items", "", 2, 20, 6),
        switch("SearchAtBottom", "/search_at_bottom", StartMenu, "Search bar at the bottom", STARTLINE_ONLY),
        number("Columns", "/columns", StartMenu, "Pinned apps per row", STARTLINE_ONLY, 3, 12, 6),
        number("TileSize", "/tile_size", StartMenu, "Pinned app size", STARTLINE_ONLY, 52, 132, 84),
        switch("ShowLabels", "/show_labels", StartMenu, "Names under pinned apps", STARTLINE_ONLY),
        switch("CompactRows", "/compact_rows", StartMenu, "Compact lists", STARTLINE_ONLY),
        entry("HorizontalPosition", "/h_anchor", StartMenu, "Where it opens across the screen", STARTLINE_ONLY, options(&[("Left", "Left"), ("Center", "Center"), ("Right", "Right"), ("FollowStart", "Beside the Start button")])),
        entry("VerticalPosition", "/v_anchor", StartMenu, "Where it opens up and down the screen", STARTLINE_ONLY, options(&[("Top", "Top"), ("Middle", "Middle"), ("Bottom", "Bottom")])),
        number("Margin", "/margin", StartMenu, "Gap from the screen edge", STARTLINE_ONLY, 0, 80, 12),
        number("Width", "/width", StartMenu, "Menu width", STARTLINE_ONLY, 420, 1200, 660),
        number("Height", "/height", StartMenu, "Menu height", STARTLINE_ONLY, 420, 1100, 720),
        // Pinned apps
        list(PINS, "/groups", Pins, "Pinned apps", "One per line: Section|Name|Target or Section|Name|Target|Arguments. Target is a file, folder, program or web address, or app:ID for an installed app. Environment variables such as %USERPROFILE% work. This replaces the person's pins and stops them changing pins."),
        // Search
        list("SearchFolders", "/roots", Search, "Folders to search", "One folder or drive per line. Environment variables such as %USERPROFILE% work."),
        list("ExcludedFolders", "/excluded_paths", Search, "Exact folders to leave out", "One folder per line, such as %USERPROFILE%\\AppData. Everything inside is left out too."),
        list("ExcludedFolderNames", "/exclude", Search, "Folder names to leave out", "One name per line. A folder with this name is left out wherever it appears."),
        list("ExcludedFileTypes", "/excluded_exts", Search, "File types to leave out", "One per line, such as .iso."),
        switch("SearchNetworkDrives", "/auto_network", Search, "Search network drives", "Includes any network drive as it gets attached. Big shares can take a while to read."),
        switch("SearchUsbDrives", "/auto_removable", Search, "Search USB drives", "Includes USB sticks and disks as they get plugged in."),
        switch("IncludeHiddenFiles", "/index_hidden", Search, "Search hidden files and folders", "Includes things Windows keeps out of sight."),
        list("ResultOrder", "/category_order", Search, "Order of the results", "One per line, first to last: Apps, Settings, Files, Web."),
        number("MaxFiles", "/max_files", Search, "Most files shown in the results", "", 5, 200, 30),
        switch("ShowFolderUnderFile", "/show_paths", Search, "Show the folder under each file", ""),
        switch("ShowSearchTime", "/show_timing", Search, "Show how long a search took", ""),
        switch("OfferWebSearch", "/web_search", Search, "Offer a web search", "A last line under the results."),
        entry("WebSearchName", "/web_name", Search, "Name of the web search", "Shown as: Search <name> for ...", Kind::Text("the web")),
        entry("WebSearchAddress", "/web_url", Search, "Web search address", "Must start with http:// or https:// and hold {} where the words go. Anything else is ignored.", Kind::Text("https://duckduckgo.com/?q={}")),
        // General
        switch(AUTOSTART, "", General, "Start with Windows", "Keeps the Start menu working after a restart."),
        switch("TakeOverStartKey", "/take_over_start", General, "Start key and button open this menu", "Shift + Start still opens the Windows menu."),
        switch("TakeOverSearchKey", "/take_over_search", General, "Start + S opens this menu too", "Other Windows shortcuts keep working."),
        switch("SkipWelcome", "/seen_welcome", General, "Skip the welcome note", "Enabled hides the note shown on the first run."),
        // Updates
        switch("AutomaticUpdates", "/auto_update", Updates, "Install updates automatically", "Startline installs new versions by itself, restarting while its menu is closed. On an install for the whole PC, the Startline Updater task installs them as SYSTEM within the hour. Disabled leaves updates to the person, or on an install for the whole PC, to the administrator."),
        switch(CHECK_FOR_UPDATES, "", Updates, "Look for updates", "Disabled stops Startline looking for new versions at all, for organizations that deploy them their own way."),
        // What people can do
        switch(WINDOWS_SETTINGS, "", Limits, "Change Windows settings from search", "Disabled removes every Windows setting from search."),
        switch(PAGES, "", Limits, "Shortcuts to Windows settings and tools", "Disabled removes search's shortcuts to Windows settings pages and tools, and the Control Panel, printer and default program links."),
        switch(FILE_BUTTONS, "", Limits, "File type buttons under search", "Disabled removes the buttons that narrow results to one kind of file."),
    ];
    for preset in SKIP_PRESETS {
        list.push(switch(&skip_key(preset.title), "", Search, &format!("Skip: {}", preset.title), preset.about));
    }

    let mut commands: Vec<(String, String)> = ACTIONS.iter().filter_map(|action| command_id(action.what).map(|id| (id.to_string(), action.name.to_string()))).collect();
    commands.push(("Command_SettingsFolder".to_string(), "Open my settings folder".to_string()));
    commands.push(("Command_ResetSettings".to_string(), "Reset all settings".to_string()));
    list.push(entry("Commands", "", Limits, "Commands people can use", "Unticked commands are removed everywhere they appear. Removing Customize Startline also closes off the settings window.", Kind::Checks(commands)));

    let menu = [
        ("Menu_RunAsAdmin", "Run as administrator"),
        ("Menu_OpenWith", "Open with"),
        ("Menu_OpenLocation", "Open file location"),
        ("Menu_Copy", "Copy path, name or address"),
        ("Menu_Uninstall", "Uninstall"),
        ("Menu_Recycle", "Move to Recycle Bin"),
        ("Menu_Properties", "Properties"),
        ("Menu_ShellMenu", "More options (the Windows menu)"),
    ];
    let menu = menu.iter().map(|(id, label)| (id.to_string(), label.to_string())).collect();
    list.push(entry("RightClickOptions", "", Limits, "Right-click options", "Unticked options are removed. Their keyboard shortcuts stop working too.", Kind::Checks(menu)));

    let windows = QuickId::ALL.iter().map(|id| (format!("Setting_{id:?}"), id.info().label.to_string())).collect();
    list.push(entry("WindowsSettingsInSearch", "", Limits, "Windows settings search can change", "Unticked settings are removed from search.", Kind::Checks(windows)));
    list
}

// ---- What the Registry says ----

/// Policy values as they are stored, before any meaning is given to them.
#[derive(Default, Debug)]
pub struct Raw {
    numbers: HashMap<String, u32>,
    texts: HashMap<String, String>,
    lists: HashMap<String, Vec<String>>,
}

impl Raw {
    /// One value for tests: a number, words, or a list of words.
    fn add_json(&mut self, name: &str, value: &Value) {
        match value {
            Value::Number(number) => {
                self.numbers.insert(name.to_string(), number.as_u64().unwrap_or(0) as u32);
            }
            Value::String(text) => {
                self.texts.insert(name.to_string(), text.clone());
            }
            Value::Array(items) => {
                self.lists.insert(name.to_string(), items.iter().filter_map(|i| i.as_str().map(str::to_string)).collect());
            }
            _ => {}
        }
    }
}

enum Stored {
    Number(u32),
    Text(String),
}

fn read_key(root: HKEY, path: &str) -> Vec<(String, Stored)> {
    let wide = platform::wide(path);
    let mut found = Vec::new();
    let mut key = HKEY::default();
    unsafe {
        if RegOpenKeyExW(root, PCWSTR(wide.as_ptr()), Some(0), KEY_READ, &mut key).is_err() {
            return found;
        }
        let mut name = vec![0u16; 16384];
        let mut data = vec![0u8; 65536];
        for index in 0.. {
            let mut name_len = name.len() as u32;
            let mut size = data.len() as u32;
            let mut kind = 0u32;
            let result = RegEnumValueW(key, index, Some(PWSTR(name.as_mut_ptr())), &mut name_len, None, Some(&mut kind), Some(data.as_mut_ptr()), Some(&mut size));
            if result == windows::Win32::Foundation::ERROR_NO_MORE_ITEMS {
                break;
            }
            if result.is_err() {
                continue;
            }
            let value_name = String::from_utf16_lossy(&name[..name_len as usize]);
            let bytes = &data[..size as usize];
            if kind == REG_DWORD.0 && bytes.len() >= 4 {
                found.push((value_name, Stored::Number(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))));
            } else if kind == REG_SZ.0 || kind == REG_EXPAND_SZ.0 {
                let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|u| *u != 0).collect();
                found.push((value_name, Stored::Text(String::from_utf16_lossy(&units))));
            }
        }
        let _ = RegCloseKey(key);
    }
    found
}

fn read_into(raw: &mut Raw, root: HKEY, path: &str) {
    merge_values(raw, read_key(root, path), |list| read_key(root, &format!("{path}\\{list}")));
}

fn merge_values(raw: &mut Raw, values: Vec<(String, Stored)>, read_list: impl Fn(&str) -> Vec<(String, Stored)>) {
    // A list says it's in force with a value of its own, so an empty list can be meant.
    for policy in all().iter().filter(|p| matches!(p.kind, Kind::List)) {
        let here = values.iter().any(|(name, value)| *name == policy.key && matches!(value, Stored::Number(1)));
        if here {
            let mut items: Vec<(u32, String)> = read_list(&policy.key)
                .into_iter()
                .filter_map(|(name, value)| match value {
                    Stored::Text(text) => Some((name.parse().unwrap_or(u32::MAX), text)),
                    Stored::Number(_) => None,
                })
                .collect();
            items.sort_by_key(|(order, _)| *order);
            raw.lists.insert(policy.key.clone(), items.into_iter().map(|(_, text)| text).collect());
        }
    }
    for (name, value) in values {
        match value {
            Stored::Number(number) => {
                raw.numbers.insert(name, number);
            }
            Stored::Text(text) => {
                raw.texts.insert(name, text);
            }
        }
    }
}

// ---- What it means ----

/// The policies in force right now.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Policies {
    fields: Vec<(&'static str, Value)>,
    skips: Vec<(usize, bool)>,
    autostart: Option<bool>,
    set: HashSet<String>,
    blocked: HashSet<String>,
}

fn expand(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%').and_then(|end| std::env::var(&after[..end]).ok().map(|value| (end, value))) {
            Some((end, value)) => {
                out.push_str(&value);
                rest = &after[end + 1..];
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn color(text: &str) -> Option<[u8; 3]> {
    let hex = text.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let part = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
    Some([part(0)?, part(2)?, part(4)?])
}

fn sections_of(items: &[String]) -> Vec<Section> {
    let known = [("pinned", SectionKind::Pinned), ("recent", SectionKind::Recent), ("frequent", SectionKind::Frequent), ("allapps", SectionKind::AllApps)];
    let mut sections: Vec<Section> = Vec::new();
    for item in items {
        let wanted = item.trim().to_lowercase();
        if let Some((_, kind)) = known.iter().find(|(name, _)| *name == wanted) {
            if !sections.iter().any(|s| s.kind == *kind) {
                sections.push(Section { kind: *kind, visible: true });
            }
        }
    }
    for (_, kind) in known {
        if !sections.iter().any(|s| s.kind == kind) {
            sections.push(Section { kind, visible: false });
        }
    }
    sections
}

fn categories_of(items: &[String]) -> Vec<Category> {
    let known = [("apps", Category::Apps), ("settings", Category::Settings), ("files", Category::Files), ("web", Category::Web)];
    let mut order: Vec<Category> = Vec::new();
    for item in items {
        let wanted = item.trim().to_lowercase();
        if let Some((_, category)) = known.iter().find(|(name, _)| *name == wanted) {
            if !order.contains(category) {
                order.push(*category);
            }
        }
    }
    for (_, category) in known {
        if !order.contains(&category) {
            order.push(category);
        }
    }
    order
}

/// "Section|Name|Target" or "Section|Name|Target|Arguments". Lines that aren't either are skipped.
fn pins_of(items: &[String]) -> Vec<PinGroup> {
    let mut groups: Vec<PinGroup> = Vec::new();
    for item in items {
        let parts: Vec<&str> = item.split('|').map(str::trim).collect();
        let (section, name, target, args) = match parts.as_slice() {
            [section, name, target] => (*section, *name, *target, ""),
            [section, name, target, args] => (*section, *name, *target, *args),
            _ => continue,
        };
        if name.is_empty() || target.is_empty() {
            continue;
        }
        let target = match target.strip_prefix("app:") {
            Some(id) => Target::App(id.to_string()),
            None => Target::Path { path: expand(target), args: expand(args) },
        };
        let section = if section.is_empty() { "Pinned" } else { section };
        let pin = Pin { name: name.to_string(), target };
        match groups.iter_mut().find(|g| g.name == section) {
            Some(group) => group.pins.push(pin),
            None => groups.push(PinGroup { name: section.to_string(), pins: vec![pin] }),
        }
    }
    if groups.is_empty() {
        groups.push(PinGroup { name: "Pinned".to_string(), pins: Vec::new() });
    }
    groups
}

impl Policies {
    /// The policies set for this person and this PC. The PC's win where both set the same one.
    pub fn read() -> Policies {
        let mut raw = Raw::default();
        read_into(&mut raw, HKEY_CURRENT_USER, KEY);
        read_into(&mut raw, HKEY_LOCAL_MACHINE, KEY);
        Policies::from_raw(&raw)
    }

    /// For a picture of what a policy does, from a file rather than the Registry.
    pub fn from_test_file(path: &Path) -> Policies {
        let mut raw = Raw::default();
        let text = std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        if let Some(Value::Object(values)) = text {
            for (name, value) in &values {
                raw.add_json(name, value);
            }
        }
        Policies::from_raw(&raw)
    }

    pub fn from_raw(raw: &Raw) -> Policies {
        let mut out = Policies::default();
        for policy in all() {
            match &policy.kind {
                Kind::Bool => {
                    let Some(number) = raw.numbers.get(&policy.key) else { continue };
                    let on = *number != 0;
                    if policy.key == AUTOSTART {
                        out.autostart = Some(on);
                    } else if let Some(at) = SKIP_PRESETS.iter().position(|p| skip_key(p.title) == policy.key) {
                        out.skips.push((at, on));
                    } else if !policy.field.is_empty() {
                        out.fields.push((policy.field, json!(on)));
                    } else if !on {
                        out.blocked.insert(policy.key.clone());
                    }
                    out.set.insert(policy.key.clone());
                }
                Kind::Number { min, max, divide, .. } => {
                    let Some(number) = raw.numbers.get(&policy.key) else { continue };
                    let number = (*number).clamp(*min, *max);
                    let value = if *divide == 1 { json!(number) } else { json!(number as f64 / *divide as f64) };
                    out.fields.push((policy.field, value));
                    out.set.insert(policy.key.clone());
                }
                Kind::Choice(items) => {
                    let Some(text) = raw.texts.get(&policy.key) else { continue };
                    if !items.iter().any(|(value, _)| value == text) {
                        continue;
                    }
                    out.fields.push((policy.field, json!(text)));
                    if policy.key == "ColorTheme" {
                        if let Some((_, theme)) = PRESETS.iter().find(|(name, _)| name == text) {
                            out.fields.push(("/theme", serde_json::to_value(theme).unwrap_or_default()));
                        }
                    }
                    out.set.insert(policy.key.clone());
                }
                Kind::Text(_) => {
                    let Some(text) = raw.texts.get(&policy.key) else { continue };
                    if policy.field == "/web_url" && !((text.starts_with("https://") || text.starts_with("http://")) && text.contains("{}")) {
                        continue;
                    }
                    out.fields.push((policy.field, json!(text)));
                    out.set.insert(policy.key.clone());
                }
                Kind::Color(_) => {
                    let Some(rgb) = raw.texts.get(&policy.key).and_then(|text| color(text)) else { continue };
                    out.fields.push((policy.field, json!(rgb)));
                    out.set.insert(policy.key.clone());
                }
                Kind::List => {
                    let Some(items) = raw.lists.get(&policy.key) else { continue };
                    let value = match policy.field {
                        "/sections" => serde_json::to_value(sections_of(items)),
                        "/category_order" => serde_json::to_value(categories_of(items)),
                        "/groups" => serde_json::to_value(pins_of(items)),
                        "/excluded_exts" => Ok(json!(items.iter().map(|e| format!(".{}", e.trim().trim_start_matches("*.").trim_start_matches('.').to_lowercase())).collect::<Vec<_>>())),
                        _ => Ok(json!(items.iter().map(|i| expand(i.trim())).collect::<Vec<_>>())),
                    };
                    let Ok(value) = value else { continue };
                    out.fields.push((policy.field, value));
                    out.set.insert(policy.key.clone());
                }
                Kind::Checks(items) => {
                    let mut any = false;
                    for (id, _) in items {
                        if let Some(number) = raw.numbers.get(&format!("Allow_{id}")) {
                            any = true;
                            if *number == 0 {
                                out.blocked.insert(id.clone());
                            }
                        }
                    }
                    if any {
                        out.set.insert(policy.key.clone());
                    }
                }
            }
        }
        // Any one color of its own means the theme is no longer a named one.
        if THEME_KEYS[1..].iter().any(|key| out.set.contains(*key)) {
            out.fields.push(("/preset", json!("Custom")));
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    pub fn is_set(&self, key: &str) -> bool {
        self.set.contains(key)
    }

    pub fn allows(&self, id: &str) -> bool {
        !self.blocked.contains(id)
    }

    /// The value a policy fixes for a saved setting, such as "/auto_update".
    pub fn fixed(&self, field: &str) -> Option<&Value> {
        self.fields.iter().find(|(f, _)| *f == field).map(|(_, value)| value)
    }

    pub fn autostart(&self) -> Option<bool> {
        self.autostart
    }

    pub fn on_page(&self, tab: Tab) -> bool {
        all().iter().any(|p| p.group.tab() == tab && self.set.contains(&p.key))
    }

    pub fn file_buttons(&self) -> bool {
        self.allows(FILE_BUTTONS)
    }

    pub fn pins_locked(&self) -> bool {
        self.is_set(PINS)
    }

    /// Dims and switches off what's inside when any of these policies is in force.
    pub fn lock<R>(&self, ui: &mut Ui, keys: &[&str], add: impl FnOnce(&mut Ui) -> R) -> R {
        let free = !keys.iter().any(|key| self.is_set(key));
        ui.add_enabled_ui(free, add).inner
    }

    /// Puts every policy's value into the settings.
    pub fn enforce(&self, cfg: &mut Config) {
        if !self.fields.is_empty() {
            if let Ok(mut value) = serde_json::to_value(&*cfg) {
                for (field, forced) in &self.fields {
                    if let Some(slot) = value.pointer_mut(field) {
                        *slot = forced.clone();
                    }
                }
                if let Ok(next) = serde_json::from_value::<Config>(value) {
                    if next != *cfg {
                        *cfg = next;
                    }
                }
            }
        }
        for (at, on) in &self.skips {
            if SKIP_PRESETS[*at].is_on(cfg) != *on {
                SKIP_PRESETS[*at].set(cfg, *on);
            }
        }
    }

    /// What to save: the settings with the person's own choice wherever a policy is in force.
    pub fn without_forced(&self, cfg: &Config, own: &Config) -> Config {
        let mut out = cfg.clone();
        if !self.fields.is_empty() {
            if let (Ok(mut value), Ok(mine)) = (serde_json::to_value(cfg), serde_json::to_value(own)) {
                for (field, _) in &self.fields {
                    if let (Some(slot), Some(original)) = (value.pointer_mut(field), mine.pointer(field)) {
                        *slot = original.clone();
                    }
                }
                if let Ok(next) = serde_json::from_value::<Config>(value) {
                    out = next;
                }
            }
        }
        for (at, _) in &self.skips {
            SKIP_PRESETS[*at].set(&mut out, SKIP_PRESETS[*at].is_on(own));
        }
        out
    }

    pub fn allows_do(&self, what: Do) -> bool {
        match what {
            Do::Open(target) => !(target.starts_with("ms-settings:") || target == "control") || self.allows(PAGES),
            other => command_id(other).is_none_or(|id| self.allows(id)),
        }
    }

    /// Whether a search result may be shown.
    pub fn allows_row(&self, kind: &RowKind) -> bool {
        match kind {
            RowKind::Run(Do::Open(_)) => self.allows(PAGES),
            RowKind::Run(what) => self.allows_do(*what),
            RowKind::Quick(id, _) => self.allows(WINDOWS_SETTINGS) && self.allows(&format!("Setting_{id:?}")) && (*id != QuickId::Autostart || self.autostart.is_none()),
            _ => true,
        }
    }

    pub fn allows_intent(&self, intent: &Intent) -> bool {
        match intent {
            Intent::Open { admin: true, .. } => self.allows("Menu_RunAsAdmin"),
            Intent::Folder(_) => self.allows("Menu_OpenLocation"),
            Intent::Copy(_) => self.allows("Menu_Copy"),
            Intent::TogglePin { .. } | Intent::MoveSection(..) | Intent::RemoveSection(_) => !self.pins_locked(),
            Intent::Show(Page::Customize(_)) => self.allows("Command_Customize"),
            Intent::Run(Do::Open("ms-settings:appsfeatures")) => self.allows("Menu_Uninstall"),
            Intent::Run(what) => self.allows_do(*what),
            Intent::Properties(_) => self.allows("Menu_Properties"),
            Intent::OpenWith(_) => self.allows("Menu_OpenWith"),
            Intent::Recycle(_) => self.allows("Menu_Recycle"),
            Intent::ShellMenu(_) => self.allows("Menu_ShellMenu"),
            _ => true,
        }
    }

    pub fn apply_autostart(&self) {
        if let Some(on) = self.autostart {
            if settings::autostart_enabled() != on {
                settings::set_autostart(on);
            }
        }
    }
}

// ---- The Group Policy templates ----

fn xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn explain(policy: &Policy) -> String {
    let what = match policy.kind {
        Kind::Bool => "Enabled turns this on and Disabled turns it off, and people can't change it. Not configured leaves it to them.",
        Kind::Checks(_) => "Enabled applies the ticks below. Disabled or Not configured allows everything.",
        _ => "Enabled sets it to the value below, and people can't change it. Disabled or Not configured leaves it to them.",
    };
    if policy.about.is_empty() { what.to_string() } else { format!("{}\n\n{what}", policy.about) }
}

/// The templates: Startline.admx, and en-US\Startline.adml with the words.
pub fn templates() -> (String, String) {
    let mut admx = String::new();
    let mut strings = String::new();
    let mut shown = String::new();
    let mut say = |id: &str, text: &str| strings.push_str(&format!("      <string id=\"{id}\">{}</string>\n", xml(text)));

    say("Supported", "Startline 0.1 and later");
    say("Cat_Startline", "Startline");
    admx.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<policyDefinitions xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" revision=\"1.0\" schemaVersion=\"1.0\" xmlns=\"http://schemas.microsoft.com/GroupPolicy/2006/07/PolicyDefinitions\">\n");
    admx.push_str("  <policyNamespaces>\n    <target prefix=\"startline\" namespace=\"Startline.Policies.Startline\"/>\n  </policyNamespaces>\n");
    admx.push_str("  <resources minRequiredRevision=\"1.0\" fallbackCulture=\"en-us\"/>\n");
    admx.push_str("  <supportedOn>\n    <definitions>\n      <definition name=\"SUPPORTED_Startline\" displayName=\"$(string.Supported)\"/>\n    </definitions>\n  </supportedOn>\n");
    admx.push_str("  <categories>\n    <category name=\"Startline\" displayName=\"$(string.Cat_Startline)\"/>\n");
    for (_, id, label) in Group::ALL {
        say(&format!("Cat_{id}"), label);
        admx.push_str(&format!("    <category name=\"{id}\" displayName=\"$(string.Cat_{id})\">\n      <parentCategory ref=\"Startline\"/>\n    </category>\n"));
    }
    admx.push_str("  </categories>\n  <policies>\n");

    for policy in all() {
        let key = &policy.key;
        let group = Group::ALL.iter().find(|(g, _, _)| *g == policy.group).map_or("", |(_, id, _)| id);
        say(&format!("P_{key}"), &policy.title);
        say(&format!("E_{key}"), &explain(policy));
        let label = xml(&format!("{}:", policy.title));
        let mut elements = String::new();
        let mut presentation = String::new();
        let mut own_value = String::new();
        let mut values = String::new();
        match &policy.kind {
            Kind::Bool => {
                own_value = format!(" valueName=\"{key}\"");
                values = "      <enabledValue>\n        <decimal value=\"1\"/>\n      </enabledValue>\n      <disabledValue>\n        <decimal value=\"0\"/>\n      </disabledValue>\n".to_string();
            }
            Kind::Number { min, max, divide, default } => {
                // Percentages are stored whole; the app divides them.
                let _ = divide;
                elements = format!("        <decimal id=\"Value\" valueName=\"{key}\" required=\"true\" minValue=\"{min}\" maxValue=\"{max}\"/>\n");
                presentation = format!("        <decimalTextBox refId=\"Value\" defaultValue=\"{default}\">{label}</decimalTextBox>\n");
            }
            Kind::Choice(items) => {
                elements.push_str(&format!("        <enum id=\"Value\" valueName=\"{key}\" required=\"true\">\n"));
                for (index, (value, text)) in items.iter().enumerate() {
                    say(&format!("I_{key}_{index}"), text);
                    elements.push_str(&format!("          <item displayName=\"$(string.I_{key}_{index})\">\n            <value>\n              <string>{}</string>\n            </value>\n          </item>\n", xml(value)));
                }
                elements.push_str("        </enum>\n");
                presentation = format!("        <dropdownList refId=\"Value\" noSort=\"true\" defaultItem=\"0\">{label}</dropdownList>\n");
            }
            Kind::Text(default) | Kind::Color(default) => {
                elements = format!("        <text id=\"Value\" valueName=\"{key}\" required=\"true\"/>\n");
                presentation = format!("        <textBox refId=\"Value\">\n          <label>{label}</label>\n          <defaultValue>{}</defaultValue>\n        </textBox>\n", xml(default));
            }
            Kind::List => {
                own_value = format!(" valueName=\"{key}\"");
                values = "      <enabledValue>\n        <decimal value=\"1\"/>\n      </enabledValue>\n      <disabledValue>\n        <delete/>\n      </disabledValue>\n".to_string();
                elements = format!("        <list id=\"Value\" key=\"{KEY}\\{key}\" valuePrefix=\"\" expandable=\"true\"/>\n");
                presentation = format!("        <listBox refId=\"Value\">{label}</listBox>\n");
            }
            Kind::Checks(items) => {
                for (id, text) in items {
                    elements.push_str(&format!("        <boolean id=\"{id}\" valueName=\"Allow_{id}\">\n          <trueValue>\n            <decimal value=\"1\"/>\n          </trueValue>\n          <falseValue>\n            <decimal value=\"0\"/>\n          </falseValue>\n        </boolean>\n"));
                    presentation.push_str(&format!("        <checkBox refId=\"{id}\" defaultChecked=\"true\">{}</checkBox>\n", xml(text)));
                }
            }
        }
        let shows = if presentation.is_empty() { String::new() } else { format!(" presentation=\"$(presentation.P_{key})\"") };
        admx.push_str(&format!("    <policy name=\"{key}\" class=\"Both\" displayName=\"$(string.P_{key})\" explainText=\"$(string.E_{key})\"{shows} key=\"{KEY}\"{own_value}>\n"));
        admx.push_str(&format!("      <parentCategory ref=\"{group}\"/>\n      <supportedOn ref=\"SUPPORTED_Startline\"/>\n{values}"));
        if !elements.is_empty() {
            admx.push_str(&format!("      <elements>\n{elements}      </elements>\n"));
        }
        admx.push_str("    </policy>\n");
        if !presentation.is_empty() {
            shown.push_str(&format!("      <presentation id=\"P_{key}\">\n{presentation}      </presentation>\n"));
        }
    }
    admx.push_str("  </policies>\n</policyDefinitions>\n");

    let adml = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<policyDefinitionResources xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" revision=\"1.0\" schemaVersion=\"1.0\" xmlns=\"http://schemas.microsoft.com/GroupPolicy/2006/07/PolicyDefinitions\">\n  <displayName>Startline</displayName>\n  <description>Settings for the Startline Start menu and search.</description>\n  <resources>\n    <stringTable>\n{strings}    </stringTable>\n    <presentationTable>\n{shown}    </presentationTable>\n  </resources>\n</policyDefinitionResources>\n"
    );
    (admx, adml)
}

/// Writes the templates into a folder laid out like Windows' own PolicyDefinitions.
pub fn write_templates(folder: &Path) -> std::io::Result<()> {
    let (admx, adml) = templates();
    std::fs::create_dir_all(folder.join("en-US"))?;
    std::fs::write(folder.join("Startline.admx"), admx)?;
    std::fs::write(folder.join("en-US").join("Startline.adml"), adml)
}

/// Policy values for other modules' tests, as the Registry would hold them.
#[cfg(test)]
pub fn tests_raw(values: Value) -> Raw {
    let mut raw = Raw::default();
    for (name, value) in values.as_object().unwrap() {
        raw.add_json(name, value);
    }
    raw
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn raw(values: Value) -> Raw {
        tests_raw(values)
    }

    fn forced(policy: &Policy) -> Option<Value> {
        match &policy.kind {
            Kind::Bool => Some(json!(1)),
            Kind::Number { min, max, .. } => Some(json!(if *min + 1 <= *max { *min + 1 } else { *min })),
            Kind::Choice(items) => Some(json!(items.last().unwrap().0)),
            Kind::Text(_) => Some(json!("https://example.com/?q={}")),
            Kind::Color(_) => Some(json!("#010203")),
            Kind::List => Some(json!(match policy.field {
                "/sections" => vec!["Recent"],
                "/category_order" => vec!["Web", "Files"],
                "/groups" => vec!["Work|Notes|C:\\notes.txt"],
                _ => vec![".test"],
            })),
            Kind::Checks(_) => None,
        }
    }

    #[test]
    fn every_saved_setting_has_a_policy() {
        let saved = serde_json::to_value(Config::default()).unwrap();
        let mut wanted: BTreeSet<String> = saved.as_object().unwrap().keys().filter(|k| !NOT_SETTINGS.contains(&k.as_str())).map(|k| format!("/{k}")).collect();
        wanted.remove("/theme");
        for part in saved["theme"].as_object().unwrap().keys() {
            wanted.insert(format!("/theme/{part}"));
        }
        let covered: BTreeSet<String> = all().iter().filter(|p| !p.field.is_empty()).map(|p| p.field.to_string()).collect();
        assert_eq!(wanted, covered);
        assert_eq!(all().iter().filter(|p| !p.field.is_empty()).count(), covered.len(), "two policies set the same setting");
    }

    #[test]
    fn every_option_has_a_policy() {
        let keys: HashSet<&str> = all().iter().map(|p| p.key.as_str()).collect();
        for preset in SKIP_PRESETS {
            assert!(keys.contains(skip_key(preset.title).as_str()), "{}", preset.title);
        }
        let checks: HashSet<&str> = all().iter().filter_map(|p| if let Kind::Checks(items) = &p.kind { Some(items) } else { None }).flatten().map(|(id, _)| id.as_str()).collect();
        for id in QuickId::ALL {
            assert!(checks.contains(format!("Setting_{id:?}").as_str()), "{id:?}");
        }
        for action in ACTIONS {
            if let Some(id) = command_id(action.what) {
                assert!(checks.contains(id), "{id}");
            }
        }
        for key in [AUTOSTART, PAGES, WINDOWS_SETTINGS, FILE_BUTTONS] {
            assert!(keys.contains(key), "{key}");
        }
    }

    #[test]
    fn every_policy_really_changes_what_it_says() {
        for policy in all() {
            let Some(value) = forced(policy) else { continue };
            let raw = raw(json!({ policy.key.clone(): value }));
            let policies = Policies::from_raw(&raw);
            assert!(policies.is_set(&policy.key), "{}", policy.key);
            if policy.field.is_empty() {
                continue;
            }
            let mut cfg = Config::default();
            cfg.seen_welcome = false;
            let before = serde_json::to_value(&cfg).unwrap();
            policies.enforce(&mut cfg);
            let after = serde_json::to_value(&cfg).unwrap();
            let expected = policies.fields.iter().find(|(f, _)| *f == policy.field).unwrap().1.clone();
            assert_eq!(after.pointer(policy.field).unwrap().as_f64().map(|n| n as f32), expected.as_f64().map(|n| n as f32), "{}", policy.key);
            if expected.as_f64().is_none() {
                assert_eq!(after.pointer(policy.field), Some(&expected), "{}", policy.key);
            }
            assert!(before.pointer(policy.field).is_some(), "{}", policy.key);
        }
    }

    #[test]
    fn switches_turn_off_as_well_as_on() {
        let policies = Policies::from_raw(&raw(json!({ "FrostedGlass": 0, "TakeOverStartKey": 0 })));
        let mut cfg = Config::default();
        policies.enforce(&mut cfg);
        assert!(!cfg.blur);
        assert!(!cfg.take_over_start);
    }

    #[test]
    fn a_policy_does_not_replace_the_persons_own_choice() {
        let mut own = Config::default();
        own.columns = 9;
        own.width = 500.0;
        let policies = Policies::from_raw(&raw(json!({ "Columns": 4, "Skip_Developerclutter": 1 })));
        let mut cfg = own.clone();
        policies.enforce(&mut cfg);
        assert_eq!(cfg.columns, 4);
        cfg.width = 700.0;
        let saved = policies.without_forced(&cfg, &own);
        assert_eq!(saved.columns, 9, "their choice is what gets saved");
        assert_eq!(saved.width, 700.0, "other changes are kept");
    }

    #[test]
    fn locked_controls_are_put_back() {
        let policies = Policies::from_raw(&raw(json!({ "Experience": "WindowsXp", "Opacity": 70, "RecentCount": 99 })));
        let mut cfg = Config::default();
        policies.enforce(&mut cfg);
        assert_eq!(cfg.experience, crate::styles::Experience::WindowsXp);
        assert_eq!(cfg.opacity, 0.7);
        assert_eq!(cfg.recent_count, 20, "kept in range");
    }

    #[test]
    fn bad_values_are_ignored() {
        let policies = Policies::from_raw(&raw(json!({ "Experience": "Vista", "Color_Accent": "red", "WebSearchAddress": "file:///c:/x" })));
        assert!(policies.is_empty());
    }

    #[test]
    fn themes_and_colors_work_together() {
        let policies = Policies::from_raw(&raw(json!({ "ColorTheme": "Moss" })));
        let mut cfg = Config::default();
        policies.enforce(&mut cfg);
        assert_eq!(cfg.preset, "Moss");
        assert_eq!(cfg.theme, PRESETS.iter().find(|(n, _)| *n == "Moss").unwrap().1);
        let policies = Policies::from_raw(&raw(json!({ "ColorTheme": "Moss", "Color_Accent": "#102030" })));
        let mut cfg = Config::default();
        policies.enforce(&mut cfg);
        assert_eq!(cfg.preset, "Custom");
        assert_eq!(cfg.theme.accent, [16, 32, 48]);
    }

    #[test]
    fn lists_keep_their_meaning() {
        let policies = Policies::from_raw(&raw(json!({
            "Sections": ["Recent", "Pinned"],
            "ResultOrder": ["Files"],
            "PinnedApps": ["Work|Notes|C:\\notes.txt", "Work|Calc|app:Calc.App", "not a pin"],
            "ExcludedFileTypes": ["ISO", "*.tmp", ".log"],
        })));
        let mut cfg = Config::default();
        policies.enforce(&mut cfg);
        assert_eq!((cfg.sections[0].kind, cfg.sections[0].visible), (SectionKind::Recent, true));
        assert_eq!((cfg.sections[2].kind, cfg.sections[2].visible), (SectionKind::Frequent, false));
        assert_eq!(cfg.category_order[0], Category::Files);
        assert_eq!(cfg.category_order.len(), 4);
        assert_eq!(cfg.groups.len(), 1);
        assert_eq!(cfg.groups[0].pins.len(), 2);
        assert_eq!(cfg.groups[0].pins[1].target, Target::App("Calc.App".to_string()));
        assert_eq!(cfg.excluded_exts, vec![".iso", ".tmp", ".log"]);
    }

    #[test]
    fn switched_off_things_are_refused() {
        let policies = Policies::from_raw(&raw(json!({
            "Allow_Command_Quit": 0,
            "Allow_Command_Lock": 1,
            "Allow_Menu_RunAsAdmin": 0,
            "Allow_Setting_Wifi": 0,
            "WindowsSettingsPages": 0,
            "StartWithWindows": 0,
        })));
        assert!(!policies.allows_do(Do::Quit));
        assert!(policies.allows_do(Do::Lock));
        assert!(!policies.allows_do(Do::Open("ms-settings:display")));
        assert!(policies.allows_do(Do::Open("shell:Personal")));
        assert!(!policies.allows_row(&RowKind::Quick(QuickId::Wifi, crate::intent::Want::Nothing)));
        assert!(policies.allows_row(&RowKind::Quick(QuickId::Volume, crate::intent::Want::Nothing)));
        assert!(!policies.allows_row(&RowKind::Quick(QuickId::Autostart, crate::intent::Want::Nothing)), "it's set by policy");
        assert!(!policies.allows_intent(&Intent::Open { name: String::new(), target: Target::path("x"), admin: true }));
        assert!(policies.allows_intent(&Intent::Open { name: String::new(), target: Target::path("x"), admin: false }));
        assert_eq!(policies.autostart(), Some(false));
    }

    #[test]
    fn pins_can_be_locked() {
        let policies = Policies::from_raw(&raw(json!({ "PinnedApps": [] })));
        assert!(policies.pins_locked());
        assert!(!policies.allows_intent(&Intent::TogglePin { name: String::new(), target: Target::path("x") }));
        let mut cfg = Config::default();
        policies.enforce(&mut cfg);
        assert_eq!(cfg.groups.len(), 1, "an empty list still leaves the place pins go");
    }

    #[test]
    fn the_registry_can_be_read() {
        // Windows' own version details are always there and hold both kinds of value. Only read.
        let found = read_key(HKEY_LOCAL_MACHINE, "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
        assert!(found.iter().any(|(name, value)| name == "ProductName" && matches!(value, Stored::Text(text) if text.starts_with("Windows"))));
        assert!(found.iter().any(|(name, value)| name == "CurrentMajorVersionNumber" && matches!(value, Stored::Number(10))));
        assert!(read_key(HKEY_CURRENT_USER, "Software\\StartlineNoSuchKey").is_empty());
    }

    #[test]
    fn values_are_understood_the_way_the_templates_write_them() {
        let values = vec![
            ("Columns".to_string(), Stored::Number(5)),
            ("Corners".to_string(), Stored::Text("Square".to_string())),
            ("FrostedGlass".to_string(), Stored::Number(0)),
            ("SearchFolders".to_string(), Stored::Number(1)),
            ("ExcludedFolders".to_string(), Stored::Number(1)),
            ("Sections".to_string(), Stored::Number(0)),
        ];
        let mut raw = Raw::default();
        merge_values(&mut raw, values, |list| {
            if list == "SearchFolders" { vec![("2".to_string(), Stored::Text("D:\\second".to_string())), ("1".to_string(), Stored::Text("C:\\first".to_string()))] } else { Vec::new() }
        });

        let policies = Policies::from_raw(&raw);
        let mut cfg = Config::default();
        policies.enforce(&mut cfg);
        assert_eq!(cfg.columns, 5);
        assert_eq!(cfg.corners, crate::config::CornerKind::Square);
        assert!(!cfg.blur);
        assert_eq!(cfg.roots, vec!["C:\\first", "D:\\second"], "in order");
        assert!(policies.is_set("ExcludedFolders"), "an empty list can be meant");
        assert!(!policies.is_set("Sections"));
    }

    #[test]
    fn nothing_set_changes_nothing() {
        let policies = Policies::from_raw(&Raw::default());
        assert!(policies.is_empty());
        let mut cfg = Config::default();
        let before = cfg.clone();
        policies.enforce(&mut cfg);
        assert_eq!(cfg, before);
        assert!(policies.allows_do(Do::Quit));
    }

    /// What's between each `open` and the next `close`, for every one in the text.
    fn each_between<'a>(mut text: &'a str, open: &str, close: char) -> Vec<&'a str> {
        let mut found = Vec::new();
        while let Some(start) = text.find(open) {
            text = &text[start + open.len()..];
            let end = text.find(close).unwrap();
            found.push(&text[..end]);
            text = &text[end..];
        }
        found
    }

    #[test]
    fn the_templates_hold_together() {
        let (admx, adml) = templates();
        for id in each_between(&admx, "$(string.", ')') {
            assert!(adml.contains(&format!("<string id=\"{id}\">")), "no words for {id}");
        }
        for id in each_between(&admx, "$(presentation.", ')') {
            assert!(adml.contains(&format!("<presentation id=\"{id}\">")), "no layout for {id}");
        }
        let names: Vec<&str> = each_between(&admx, "<policy name=\"", '"');
        assert_eq!(names.len(), all().len());
        let unique: HashSet<&&str> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "two policies with one name");
        for policy in all() {
            assert!(admx.contains(&format!("<policy name=\"{}\"", policy.key)));
        }
        for id in each_between(&adml, "refId=\"", '"') {
            assert!(admx.contains(&format!(" id=\"{id}\"")), "layout for nothing: {id}");
        }
        let ids: Vec<&str> = each_between(&adml, "<string id=\"", '"');
        let unique: HashSet<&&str> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "two strings with one id");
    }

    #[test]
    fn the_template_files_are_up_to_date() {
        let (admx, adml) = templates();
        let same = |file: &str, text: &str| text.replace("\r\n", "\n") == file.replace("\r\n", "\n");
        assert!(same(include_str!("../PolicyDefinitions/Startline.admx"), &admx), "run startline --write-policy-templates PolicyDefinitions");
        assert!(same(include_str!("../PolicyDefinitions/en-US/Startline.adml"), &adml), "run startline --write-policy-templates PolicyDefinitions");
    }
}
