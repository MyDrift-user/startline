//! How policies are read from the Registry, and what they mean for the settings.

use super::*;
use crate::config::{Category, Config, Pin, PinGroup, ResultKind, Section, SectionKind, Target};
use crate::i18n::tr;
use crate::platform;
use crate::search::RowKind;
use crate::settings;
use crate::ui::{Intent, Page};
use egui::Ui;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use windows::Win32::System::Registry::*;
use windows::core::{PCWSTR, PWSTR};

// ---- What the Registry says ----

/// Policy values as they are stored, before any meaning is given to them.
#[derive(Default, Debug)]
pub struct Raw {
    numbers: HashMap<String, u32>,
    texts: HashMap<String, String>,
    lists: HashMap<String, Vec<String>>,
}

impl Raw {
    /// One value for tests and pictures: a number, words, or a list of words.
    pub(crate) fn add_json(&mut self, name: &str, value: &Value) {
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

pub(crate) enum Stored {
    Number(u32),
    Text(String),
}

pub(crate) fn read_key(root: HKEY, path: &str) -> Vec<(String, Stored)> {
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

pub(crate) fn merge_values(raw: &mut Raw, values: Vec<(String, Stored)>, read_list: impl Fn(&str) -> Vec<(String, Stored)>) {
    // A list says it's in force with a value of its own, so an empty list can be meant.
    for policy in all().iter().filter(|p| matches!(p.kind, Kind::List | Kind::AddList)) {
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

/// What one set of policies says: the fixed ones, or the defaults.
#[derive(Clone, Default, PartialEq, Debug)]
pub(crate) struct Parsed {
    pub(crate) fields: Vec<(&'static str, Value)>,
    /// Items added to the person's own list in these settings, which they can't remove.
    pub(crate) added: Vec<(&'static str, Vec<String>)>,
    pub(crate) skips: Vec<(usize, bool)>,
    pub(crate) autostart: Option<bool>,
    pub(crate) set: HashSet<String>,
    pub(crate) blocked: HashSet<String>,
}

/// The policies in force right now.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Policies {
    pub(crate) fixed: Parsed,
    pub(crate) defaults: Parsed,
}

/// `%NAME%` replaced by that environment variable; an unknown one stays as written.
pub fn expand(text: &str) -> String {
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

/// The kinds named, in that order and shown; the rest after them and never shown.
fn results_of(items: &[String]) -> Vec<ResultKind> {
    let mut results: Vec<ResultKind> = Vec::new();
    for kind in items.iter().filter_map(|item| Category::from_name(item)) {
        if !results.iter().any(|r| r.kind == kind) {
            results.push(ResultKind { kind, on: true });
        }
    }
    for kind in Category::ALL {
        if !results.iter().any(|r| r.kind == kind) {
            results.push(ResultKind { kind, on: false });
        }
    }
    results
}

/// A file type as the settings keep it: lower case, with the dot.
pub fn file_type(text: &str) -> String {
    format!(".{}", text.trim().trim_start_matches("*.").trim_start_matches('.').to_lowercase())
}

/// The person's list for a setting an AddList policy adds to.
fn list_mut<'a>(cfg: &'a mut Config, field: &str) -> Option<&'a mut Vec<String>> {
    match field {
        "/excluded_paths" => Some(&mut cfg.excluded_paths),
        "/exclude" => Some(&mut cfg.exclude),
        "/excluded_exts" => Some(&mut cfg.excluded_exts),
        "/skip_patterns" => Some(&mut cfg.skip_patterns),
        _ => None,
    }
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
        let section = if section.is_empty() { "Pinned" } else { section }; // i18n-ignore: the first section's saved name
        let pin = Pin { name: name.to_string(), target };
        match groups.iter_mut().find(|g| g.name == section) {
            Some(group) => group.pins.push(pin),
            None => groups.push(PinGroup { name: section.to_string(), pins: vec![pin] }),
        }
    }
    if groups.is_empty() {
        groups.push(PinGroup { name: "Pinned".to_string(), pins: Vec::new() }); // i18n-ignore: the first section's saved name
    }
    groups
}

/// The saved settings with these values put in, if they still make sense together.
fn put_fields<'a>(cfg: &mut Config, fields: impl Iterator<Item = &'a (&'static str, Value)>) {
    let Ok(mut value) = serde_json::to_value(&*cfg) else { return };
    let mut any = false;
    for (field, forced) in fields {
        if let Some(slot) = value.pointer_mut(field) {
            *slot = forced.clone();
            any = true;
        }
    }
    if any {
        if let Ok(next) = serde_json::from_value::<Config>(value) {
            if next != *cfg {
                *cfg = next;
            }
        }
    }
}

/// How a preset switch is remembered as chosen.
fn skip_choice(at: usize) -> String {
    format!("skip:{}", SKIP_PRESETS[at].key)
}

impl Parsed {
    /// What the values mean. `defaults` reads them as defaults: a list an administrator adds to is then the person's
    /// starting list, and what isn't a setting (tick boxes, Look for updates) doesn't count.
    fn from_raw(raw: &Raw, defaults: bool) -> Parsed {
        let mut out = Parsed::default();
        for policy in all() {
            if defaults && !policy.has_default() {
                continue;
            }
            match &policy.kind {
                Kind::Bool => {
                    let Some(number) = raw.numbers.get(&policy.key) else { continue };
                    let on = *number != 0;
                    if policy.key == AUTOSTART {
                        out.autostart = Some(on);
                    } else if let Some(at) = SKIP_PRESETS.iter().position(|p| p.key == policy.key) {
                        out.skips.push((at, on));
                    } else if !policy.field.is_empty() {
                        out.fields.push((policy.field, json!(on)));
                        // Turned off for everyone, these also take away what they lead to elsewhere.
                        if !on && !defaults && [PAGES, WINDOWS_SETTINGS].contains(&policy.key.as_str()) {
                            out.blocked.insert(policy.key.clone());
                        }
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
                    let Some((value, _)) = items.iter().find(|(value, _)| value.eq_ignore_ascii_case(text.trim())) else { continue };
                    out.fields.push((policy.field, json!(value)));
                    if policy.key == "ColorTheme" {
                        if let Some((_, theme)) = PRESETS.iter().find(|(name, _)| name == value) {
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
                        "/results" => serde_json::to_value(results_of(items)),
                        "/groups" => serde_json::to_value(pins_of(items)),
                        _ => Ok(json!(items.iter().map(|i| expand(i.trim())).collect::<Vec<_>>())),
                    };
                    let Ok(value) = value else { continue };
                    out.fields.push((policy.field, value));
                    out.set.insert(policy.key.clone());
                }
                Kind::AddList => {
                    let Some(items) = raw.lists.get(&policy.key) else { continue };
                    let items: Vec<String> = items
                        .iter()
                        .map(|i| i.trim())
                        .filter(|i| !i.is_empty())
                        .map(|i| match policy.field {
                            "/excluded_exts" => file_type(i),
                            // a regular expression's own % and \ mean something
                            "/skip_patterns" => i.to_string(),
                            _ => expand(i),
                        })
                        .collect();
                    if defaults {
                        out.fields.push((policy.field, json!(items)));
                    } else {
                        out.added.push((policy.field, items));
                    }
                    out.set.insert(policy.key.clone());
                }
                Kind::Checks(items) => {
                    let mut any = false;
                    for (id, _) in items {
                        if let Some(number) = raw.numbers.get(&format!("Allow_{id}")) { // i18n-ignore: registry value name
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
            out.fields.push(("/preset", json!("Custom"))); // i18n-ignore: saved name of an own theme
        }
        out
    }
}

impl Policies {
    /// The policies set for this person and this PC. The PC's win where both set the same one.
    pub fn read() -> Policies {
        let (mut fixed, mut defaults) = (Raw::default(), Raw::default());
        read_into(&mut fixed, HKEY_CURRENT_USER, KEY);
        read_into(&mut fixed, HKEY_LOCAL_MACHINE, KEY);
        read_into(&mut defaults, HKEY_CURRENT_USER, RECOMMENDED);
        read_into(&mut defaults, HKEY_LOCAL_MACHINE, RECOMMENDED);
        Policies::from_raws(&fixed, &defaults)
    }

    /// For a picture of what a policy does, from a file rather than the Registry. Defaults go in an object of their own
    /// named Recommended.
    pub fn from_test_file(path: &Path) -> Policies {
        let (mut fixed, mut defaults) = (Raw::default(), Raw::default());
        let text = std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        if let Some(Value::Object(values)) = text {
            for (name, value) in &values {
                match (name.as_str(), value) {
                    ("Recommended", Value::Object(inner)) => inner.iter().for_each(|(name, value)| defaults.add_json(name, value)), // i18n-ignore: registry value name
                    _ => fixed.add_json(name, value),
                }
            }
        }
        Policies::from_raws(&fixed, &defaults)
    }

    /// Fixed policies only.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn from_raw(raw: &Raw) -> Policies {
        Policies::from_raws(raw, &Raw::default())
    }

    pub fn from_raws(fixed: &Raw, defaults: &Raw) -> Policies {
        Policies { fixed: Parsed::from_raw(fixed, false), defaults: Parsed::from_raw(defaults, true) }
    }

    pub fn is_empty(&self) -> bool {
        self.fixed.set.is_empty() && self.defaults.set.is_empty()
    }

    /// Whether a policy fixes this, so the person can't change it.
    pub fn is_set(&self, key: &str) -> bool {
        self.fixed.set.contains(key)
    }

    pub fn allows(&self, id: &str) -> bool {
        !self.fixed.blocked.contains(id)
    }

    /// The value a policy fixes for a saved setting, such as "/auto_update".
    pub fn fixed(&self, field: &str) -> Option<&Value> {
        self.fixed.fields.iter().find(|(f, _)| *f == field).map(|(_, value)| value)
    }

    pub fn autostart(&self) -> Option<bool> {
        self.fixed.autostart
    }

    /// Start with Windows as it's set the first time Startline runs: fixed, or the administrator's default.
    pub fn first_autostart(&self) -> Option<bool> {
        self.fixed.autostart.or(self.defaults.autostart)
    }

    pub fn on_page(&self, tab: Tab) -> bool {
        all().iter().any(|p| p.group.tab() == tab && self.fixed.set.contains(&p.key))
    }

    pub fn pins_locked(&self) -> bool {
        self.is_set(PINS)
    }

    /// Whether the pins come from an administrator, fixed or as a default, rather than from the apps Startline finds.
    pub fn sets_pins(&self) -> bool {
        self.fixed.set.contains(PINS) || self.defaults.set.contains(PINS)
    }

    /// Dims and switches off what's inside when any of these policies is in force, and says so.
    pub fn lock<R>(&self, ui: &mut Ui, keys: &[&str], add: impl FnOnce(&mut Ui) -> R) -> R {
        let free = !keys.iter().any(|key| self.is_set(key));
        let inner = ui.add_enabled_ui(free, add);
        if !free {
            let quiet = ui.visuals().weak_text_color();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(egui::RichText::new('\u{E72E}'.to_string()).font(crate::widgets::icon_font(11.0)).color(quiet));
                ui.label(egui::RichText::new(tr("Managed by your organization")).size(11.5).color(quiet));
            })
            .response
            .on_hover_text(tr("An administrator set this with a policy, so it can't be changed here."));
        }
        inner.inner
    }

    /// Remembers the settings the person changed themselves since `before`, so the administrator's defaults for them
    /// stop applying. Presets count too: they change settings as if by hand.
    pub fn note_choices(&self, cfg: &mut Config, before: &Config) {
        if self.defaults.fields.is_empty() && self.defaults.skips.is_empty() {
            return;
        }
        let (Ok(now), Ok(then)) = (serde_json::to_value(&*cfg), serde_json::to_value(before)) else { return };
        let mut chosen = Vec::new();
        for (field, _) in &self.defaults.fields {
            if now.pointer(field) != then.pointer(field) {
                chosen.push(field.to_string());
            }
        }
        for (at, _) in &self.defaults.skips {
            if SKIP_PRESETS[*at].is_on(cfg) != SKIP_PRESETS[*at].is_on(before) {
                chosen.push(skip_choice(*at));
            }
        }
        for field in chosen {
            if !cfg.chosen.contains(&field) {
                cfg.chosen.push(field);
            }
        }
    }

    /// Puts every policy's value into the settings: the defaults where the person hasn't chosen their own, then the
    /// fixed ones.
    pub fn enforce(&self, cfg: &mut Config) {
        let open: Vec<(&'static str, Value)> = self.defaults.fields.iter().filter(|(field, _)| !cfg.chosen.iter().any(|c| c == field)).cloned().collect();
        put_fields(cfg, open.iter().chain(self.fixed.fields.iter()));
        for (at, on) in &self.defaults.skips {
            if !cfg.chosen.contains(&skip_choice(*at)) && SKIP_PRESETS[*at].is_on(cfg) != *on {
                SKIP_PRESETS[*at].set(cfg, *on);
            }
        }
        for (at, on) in &self.fixed.skips {
            if SKIP_PRESETS[*at].is_on(cfg) != *on {
                SKIP_PRESETS[*at].set(cfg, *on);
            }
        }
        for (field, items) in &self.fixed.added {
            let Some(list) = list_mut(cfg, field) else { continue };
            for item in items {
                if !list.iter().any(|have| have.eq_ignore_ascii_case(item)) {
                    list.push(item.clone());
                }
            }
        }
    }

    /// What a policy adds to this list, such as "/exclude", which the person can't take out.
    pub fn added(&self, field: &str) -> &[String] {
        self.fixed.added.iter().find(|(f, _)| *f == field).map_or(&[], |(_, items)| items.as_slice())
    }

    /// What to save: the settings with the person's own choice wherever a policy is in force. A default they haven't
    /// changed isn't saved as theirs.
    pub fn without_forced(&self, cfg: &Config, own: &Config) -> Config {
        let mut out = cfg.clone();
        let theirs = |field: &str| cfg.chosen.iter().any(|c| c == field);
        let restore: Vec<&'static str> = self.fixed.fields.iter().map(|(f, _)| *f).chain(self.defaults.fields.iter().map(|(f, _)| *f).filter(|f| !theirs(f))).collect();
        if !restore.is_empty() {
            if let (Ok(mut value), Ok(mine)) = (serde_json::to_value(&out), serde_json::to_value(own)) {
                for field in restore {
                    if let (Some(slot), Some(original)) = (value.pointer_mut(field), mine.pointer(field)) {
                        *slot = original.clone();
                    }
                }
                if let Ok(next) = serde_json::from_value::<Config>(value) {
                    out = next;
                }
            }
        }
        for (at, _) in &self.fixed.skips {
            SKIP_PRESETS[*at].set(&mut out, SKIP_PRESETS[*at].is_on(own));
        }
        for (at, _) in &self.defaults.skips {
            if !theirs(&skip_choice(*at)) {
                SKIP_PRESETS[*at].set(&mut out, SKIP_PRESETS[*at].is_on(own));
            }
        }
        // The administrator's items aren't the person's, unless they had them too.
        let mut own = own.clone();
        for (field, items) in &self.fixed.added {
            let mine = list_mut(&mut own, field).map(|l| l.clone()).unwrap_or_default();
            if let Some(list) = list_mut(&mut out, field) {
                list.retain(|item| mine.iter().any(|m| m.eq_ignore_ascii_case(item)) || !items.iter().any(|a| a.eq_ignore_ascii_case(item)));
            }
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
            RowKind::Quick(id, _) => self.allows(WINDOWS_SETTINGS) && self.allows(&format!("Setting_{id:?}")) && (*id != QuickId::Autostart || self.fixed.autostart.is_none()), // i18n-ignore: registry value name
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
        if let Some(on) = self.fixed.autostart {
            if settings::autostart_enabled() != on {
                settings::set_autostart(on);
            }
        }
    }
}

/// Reads the policies every half minute on a thread of its own, so `gpupdate` and a script's changes reach a menu
/// that's out of sight too. What it finds waits in the slot, and the window is woken to take it.
pub fn watch(ctx: egui::Context, slot: std::sync::Arc<std::sync::Mutex<Option<Policies>>>, mut last: Policies) {
    let every = std::env::var("STARTLINE_TEST_POLICY_POLL_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(30_000);
    let _ = std::thread::Builder::new().name("policy".into()).spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(every));
            let fresh = Policies::read();
            if fresh != last {
                last = fresh.clone();
                *slot.lock().unwrap() = Some(fresh);
                ctx.request_repaint();
            }
        }
    });
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
