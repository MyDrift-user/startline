//! Decides which folders search looks in: places that are ticked, folders unticked inside
//! them, and skip rules. Plain logic with no disk access, so every case can be tested.

use crate::config::Config;

/// A ready-made skip rule shown as one switch.
pub struct SkipPreset {
    /// The policy that turns it on or off for everyone.
    pub key: &'static str,
    pub title: &'static str,
    pub about: &'static str,
    pub names: &'static [&'static str],
    pub exts: &'static [&'static str],
}

pub const SKIP_PRESETS: &[SkipPreset] = &[
    SkipPreset {
        key: "SearchSkipAppData",
        title: "App data",
        about: "Settings and caches programs keep in AppData.",
        names: &["AppData"],
        exts: &[],
    },
    SkipPreset {
        key: "SearchSkipWindows",
        title: "Windows and installed programs",
        about: "Windows, Program Files and other system folders.",
        names: &["Windows", "Program Files", "Program Files (x86)", "ProgramData", "$Recycle.Bin", "System Volume Information", "$WinREAgent", "Recovery"],
        exts: &[],
    },
    SkipPreset {
        key: "SearchSkipDeveloperFiles",
        title: "Developer clutter",
        about: "node_modules, .git, build output and similar.",
        names: &["node_modules", ".git", ".svn", "target", "__pycache__", ".cache", ".cargo", ".rustup", ".npm", ".gradle", ".venv", "venv"],
        exts: &[],
    },
    SkipPreset {
        key: "SearchSkipCloudFolders",
        title: "Cloud folders",
        about: "OneDrive, Dropbox and Google Drive folders.",
        names: &["OneDrive", "Dropbox", "Google Drive", "iCloudDrive"],
        exts: &[],
    },
    SkipPreset {
        key: "SearchSkipTempFiles",
        title: "Temporary and log files",
        about: "Leftovers such as .tmp, .log and .bak.",
        names: &[],
        exts: &[".tmp", ".temp", ".log", ".bak", ".dmp"],
    },
    SkipPreset {
        key: "SearchSkipProgramParts",
        title: "Program parts",
        about: "Files you never open yourself, such as .dll and .sys.",
        names: &[],
        exts: &[".dll", ".sys", ".mui", ".cat", ".pdb", ".obj", ".lib"],
    },
];

fn holds(list: &[String], item: &str) -> bool {
    list.iter().any(|entry| entry.eq_ignore_ascii_case(item))
}

impl SkipPreset {
    pub fn is_on(&self, cfg: &Config) -> bool {
        self.names.iter().all(|n| holds(&cfg.exclude, n)) && self.exts.iter().all(|e| holds(&cfg.excluded_exts, e))
    }

    pub fn set(&self, cfg: &mut Config, on: bool) {
        apply_list(&mut cfg.exclude, self.names, on);
        apply_list(&mut cfg.excluded_exts, self.exts, on);
    }
}

fn apply_list(list: &mut Vec<String>, items: &[&str], on: bool) {
    list.retain(|entry| !items.iter().any(|item| entry.eq_ignore_ascii_case(item)));
    if on {
        list.extend(items.iter().map(|item| item.to_string()));
    }
}

/// True when this rule belongs to a switch rather than being one the person typed.
pub fn in_a_preset(item: &str) -> bool {
    SKIP_PRESETS.iter().any(|p| p.names.iter().chain(p.exts).any(|n| n.eq_ignore_ascii_case(item)))
}

fn clean(path: &str) -> &str {
    let trimmed = path.trim_end_matches('\\');
    // A drive keeps its backslash in the settings, but compares without it.
    if trimmed.is_empty() { path } else { trimmed }
}

pub fn same_path(a: &str, b: &str) -> bool {
    clean(a).eq_ignore_ascii_case(clean(b))
}

/// `inner` is somewhere below `outer`.
pub fn is_inside(inner: &str, outer: &str) -> bool {
    let (inner, outer) = (clean(inner), clean(outer));
    inner.len() > outer.len()
        && inner.is_char_boundary(outer.len())
        && inner[..outer.len()].eq_ignore_ascii_case(outer)
        && inner.as_bytes()[outer.len()] == b'\\'
}

/// The folder above, or `None` at the top of a drive or network share.
pub fn parent_of(path: &str) -> Option<&str> {
    let path = clean(path);
    let (parent, _) = path.rsplit_once('\\')?;
    if parent.is_empty() || parent == "\\" {
        return None;
    }
    // "\\server\share" is the top of a network location.
    if let Some(rest) = path.strip_prefix("\\\\") {
        if rest.matches('\\').count() < 2 {
            return None;
        }
    }
    Some(parent)
}

fn name_of(path: &str) -> &str {
    let path = clean(path);
    path.rsplit('\\').next().unwrap_or(path)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tick {
    On,
    Off,
    /// Searched, but with folders left out inside; or not searched, with some added inside.
    Some,
}

/// Everything needed to answer "is this folder searched?".
pub struct Scope<'a> {
    /// Ticked places, including drives that are on automatically.
    pub roots: &'a [String],
    pub excluded_paths: &'a [String],
    pub excluded_names: &'a [String],
    pub include_hidden: bool,
    pub is_hidden: &'a dyn Fn(&str) -> bool,
}

impl Scope<'_> {
    /// A rule leaves this folder out, whatever the folders around it say.
    pub fn rule_blocks(&self, path: &str) -> bool {
        holds(self.excluded_names, name_of(path)) || (!self.include_hidden && (self.is_hidden)(path))
    }

    pub fn is_searched(&self, path: &str) -> bool {
        if self.excluded_paths.iter().any(|p| same_path(p, path)) {
            return false;
        }
        if self.roots.iter().any(|r| same_path(r, path)) {
            return true;
        }
        if self.rule_blocks(path) {
            return false;
        }
        parent_of(path).is_some_and(|parent| self.is_searched(parent))
    }

    pub fn tick(&self, path: &str) -> Tick {
        if self.is_searched(path) {
            if self.excluded_paths.iter().any(|p| is_inside(p, path)) { Tick::Some } else { Tick::On }
        } else if self.roots.iter().any(|r| is_inside(r, path)) {
            Tick::Some
        } else {
            Tick::Off
        }
    }

    /// The places to actually read: anything already reached through another place is
    /// dropped, so nothing is read or listed twice.
    pub fn places_to_read(&self) -> Vec<String> {
        let mut places: Vec<String> = Vec::new();
        for root in self.roots {
            if self.excluded_paths.iter().any(|p| same_path(p, root)) || places.iter().any(|p| same_path(p, root)) {
                continue;
            }
            let reached_anyway = !self.rule_blocks(root) && parent_of(root).is_some_and(|parent| self.is_searched(parent));
            if !reached_anyway {
                places.push(root.clone());
            }
        }
        places
    }
}

/// Flips a folder's tick. `auto_roots` are places that are on without being listed in the
/// settings, such as a network drive that is included automatically.
pub fn toggle(cfg: &mut Config, auto_roots: &[String], path: &str, is_hidden: &dyn Fn(&str) -> bool) {
    let was_on = {
        let all = all_roots(cfg, auto_roots);
        let scope = Scope { roots: &all, excluded_paths: &cfg.excluded_paths, excluded_names: &cfg.exclude, include_hidden: cfg.skip_attrs() == 0, is_hidden };
        scope.is_searched(path)
    };
    if was_on {
        // Off means everything inside too, so earlier choices in there no longer apply.
        cfg.roots.retain(|r| !same_path(r, path) && !is_inside(r, path));
        cfg.excluded_paths.retain(|p| !is_inside(p, path));
    } else {
        cfg.excluded_paths.retain(|p| !same_path(p, path));
    }
    let all = all_roots(cfg, auto_roots);
    let scope = Scope { roots: &all, excluded_paths: &cfg.excluded_paths, excluded_names: &cfg.exclude, include_hidden: cfg.skip_attrs() == 0, is_hidden };
    let now_on = scope.is_searched(path);
    if was_on && now_on {
        cfg.excluded_paths.push(path.to_string());
    } else if !was_on && !now_on {
        cfg.roots.push(path.to_string());
    }
}

pub fn all_roots(cfg: &Config, auto_roots: &[String]) -> Vec<String> {
    let mut all = cfg.roots.clone();
    all.extend(auto_roots.iter().cloned());
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "C:\\Users\\me";

    fn cfg() -> Config {
        Config { roots: vec![HOME.to_string()], exclude: vec!["AppData".to_string()], excluded_paths: vec![], excluded_exts: vec![], ..Default::default() }
    }

    fn scope_of<'a>(cfg: &'a Config, all: &'a [String]) -> Scope<'a> {
        Scope { roots: all, excluded_paths: &cfg.excluded_paths, excluded_names: &cfg.exclude, include_hidden: cfg.skip_attrs() == 0, is_hidden: &|_| false }
    }

    fn searched(cfg: &Config, path: &str) -> bool {
        let all = all_roots(cfg, &[]);
        scope_of(cfg, &all).is_searched(path)
    }

    fn tick(cfg: &Config, path: &str) -> Tick {
        let all = all_roots(cfg, &[]);
        scope_of(cfg, &all).tick(path)
    }

    #[test]
    fn home_folder_is_searched_except_app_data() {
        let cfg = cfg();
        assert!(searched(&cfg, "C:\\Users\\me\\Documents\\Taxes"));
        assert!(!searched(&cfg, "C:\\Users\\me\\AppData"));
        assert!(!searched(&cfg, "C:\\Users\\me\\AppData\\Local"));
        assert!(!searched(&cfg, "C:\\Users\\other"));
        assert!(!searched(&cfg, "D:\\"));
    }

    #[test]
    fn unticking_a_folder_leaves_the_rest_of_the_home_folder_on() {
        let mut cfg = cfg();
        toggle(&mut cfg, &[], "C:\\Users\\me\\Secret", &|_| false);
        assert!(!searched(&cfg, "C:\\Users\\me\\Secret\\file"));
        assert!(searched(&cfg, "C:\\Users\\me\\Documents"));
        assert_eq!(tick(&cfg, HOME), Tick::Some);
        toggle(&mut cfg, &[], "C:\\Users\\me\\Secret", &|_| false);
        assert!(searched(&cfg, "C:\\Users\\me\\Secret"));
        assert!(cfg.excluded_paths.is_empty());
        assert_eq!(tick(&cfg, HOME), Tick::On);
    }

    #[test]
    fn ticking_a_folder_a_rule_skips_adds_it_on_its_own() {
        let mut cfg = cfg();
        toggle(&mut cfg, &[], "C:\\Users\\me\\AppData\\Roaming", &|_| false);
        assert!(searched(&cfg, "C:\\Users\\me\\AppData\\Roaming\\x"));
        assert!(!searched(&cfg, "C:\\Users\\me\\AppData\\Local"));
        assert_eq!(tick(&cfg, "C:\\Users\\me\\AppData"), Tick::Some);
    }

    #[test]
    fn hidden_folders_count_as_a_rule_until_hidden_files_are_included() {
        let mut cfg = cfg();
        let hidden = |p: &str| p.ends_with("\\Vault");
        let all = all_roots(&cfg, &[]);
        let scope = Scope { roots: &all, excluded_paths: &[], excluded_names: &[], include_hidden: false, is_hidden: &hidden };
        assert!(!scope.is_searched("C:\\Users\\me\\Vault"));
        cfg.skip_hidden = false;
        cfg.skip_system = false;
        let scope = Scope { roots: &all, excluded_paths: &[], excluded_names: &[], include_hidden: true, is_hidden: &hidden };
        assert!(scope.is_searched("C:\\Users\\me\\Vault"));
    }

    #[test]
    fn turning_a_place_off_forgets_the_choices_inside_it() {
        let mut cfg = cfg();
        toggle(&mut cfg, &[], "C:\\Users\\me\\Secret", &|_| false);
        toggle(&mut cfg, &[], HOME, &|_| false);
        assert!(cfg.roots.is_empty());
        assert!(cfg.excluded_paths.is_empty());
        assert_eq!(tick(&cfg, HOME), Tick::Off);
        toggle(&mut cfg, &[], HOME, &|_| false);
        assert_eq!(tick(&cfg, HOME), Tick::On);
        assert_eq!(cfg.roots, vec![HOME.to_string()]);
    }

    #[test]
    fn a_drive_that_is_on_automatically_can_still_be_left_out() {
        let mut cfg = cfg();
        let auto = vec!["Z:\\".to_string()];
        let on = |cfg: &Config| {
            let all = all_roots(cfg, &auto);
            scope_of(cfg, &all).is_searched("Z:\\Projects")
        };
        assert!(on(&cfg));
        toggle(&mut cfg, &auto, "Z:\\", &|_| false);
        assert!(!on(&cfg));
        let all = all_roots(&cfg, &auto);
        assert!(scope_of(&cfg, &all).places_to_read().iter().all(|p| p != "Z:\\"));
        toggle(&mut cfg, &auto, "Z:\\", &|_| false);
        assert!(on(&cfg));
        assert!(cfg.excluded_paths.is_empty());
        assert!(!cfg.roots.iter().any(|r| r == "Z:\\"), "it stays automatic");
    }

    #[test]
    fn nothing_is_read_twice() {
        let mut cfg = cfg();
        cfg.roots.push("C:\\Users\\me\\Documents".to_string());
        cfg.roots.push("c:\\users\\me".to_string());
        cfg.roots.push("C:\\Users\\me\\AppData\\Roaming".to_string());
        let all = all_roots(&cfg, &[]);
        let places = scope_of(&cfg, &all).places_to_read();
        assert_eq!(places, vec![HOME.to_string(), "C:\\Users\\me\\AppData\\Roaming".to_string()]);
    }

    #[test]
    fn network_paths_and_drives_have_a_top() {
        assert_eq!(parent_of("\\\\server\\share\\docs"), Some("\\\\server\\share"));
        assert_eq!(parent_of("\\\\server\\share"), None);
        assert_eq!(parent_of("Z:\\"), None);
        assert_eq!(parent_of("Z:\\work"), Some("Z:"));
        assert!(same_path("Z:", "z:\\"));
        assert!(is_inside("Z:\\work", "Z:\\"));
        assert!(!is_inside("C:\\Users\\me2", "C:\\Users\\me"));
    }

    #[test]
    fn a_skip_switch_is_on_only_when_all_its_rules_are() {
        let mut cfg = cfg();
        let dev = SKIP_PRESETS.iter().find(|p| p.title == "Developer clutter").unwrap();
        assert!(!dev.is_on(&cfg));
        dev.set(&mut cfg, true);
        assert!(dev.is_on(&cfg));
        cfg.exclude.retain(|n| n != ".git");
        assert!(!dev.is_on(&cfg));
        dev.set(&mut cfg, false);
        assert_eq!(cfg.exclude, vec!["AppData".to_string()], "other rules stay");
        assert!(in_a_preset(".TMP"));
        assert!(!in_a_preset("my-folder"));
    }
}
