//! The list of installed apps, the same one Windows uses for its own "All apps" list,
//! so both classic programs and Store apps show up.

use crate::matching;
use crate::platform;
use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use std::sync::{Arc, RwLock};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::System::Com::*;
use windows::Win32::UI::Shell::*;
use windows::core::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct App {
    pub name: String,
    /// The Windows app id, used to launch it.
    pub id: String,
    /// The program behind it when Windows knows (empty for most Store apps).
    pub path: String,
    #[serde(skip)]
    pub lower: String,
    /// Other words that should find it, such as the program's file name.
    #[serde(skip)]
    pub alias: String,
}

impl App {
    fn prepare(&mut self) {
        self.lower = matching::lower(&self.name);
        let stem = self.path.rsplit('\\').next().unwrap_or("");
        let stem = stem.rsplit_once('.').map(|(s, _)| s).unwrap_or(stem);
        self.alias = matching::lower(&format!("{stem} {}", package_name(&self.id))).trim().to_string();
    }
}

/// A Store app's id keeps its English name on every Windows language, so "notepad" still
/// finds it when Windows calls it "Editor": Microsoft.WindowsNotepad_8wekyb3d8bbwe!App.
fn package_name(id: &str) -> &str {
    let Some((package, _)) = id.split_once('!') else { return "" };
    let Some((name, _)) = package.split_once('_') else { return "" };
    let name = name.rsplit('.').next().unwrap_or(name);
    name.strip_prefix("Windows").filter(|rest| !rest.is_empty()).unwrap_or(name) // i18n-ignore: part of app ids
}

fn cache_path() -> std::path::PathBuf {
    crate::config::data_dir().join("apps.json")
}

/// Starts with the saved list so the menu is complete at once, then refreshes it.
pub fn start(ctx: egui::Context) -> Arc<RwLock<Vec<App>>> {
    let mut cached: Vec<App> = std::fs::read(cache_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    cached.iter_mut().for_each(App::prepare);
    let list = Arc::new(RwLock::new(cached));
    let shared = list.clone();
    std::thread::Builder::new()
        .name("apps".into())
        .spawn(move || {
            platform::com_init();
            loop {
                if let Some(fresh) = enumerate() {
                    if let Ok(bytes) = serde_json::to_vec(&fresh) {
                        let _ = std::fs::write(cache_path(), bytes);
                    }
                    if let Ok(mut current) = shared.write() {
                        *current = fresh;
                    }
                    ctx.request_repaint();
                }
                // New installs show up within a few minutes without any polling cost worth noticing.
                std::thread::sleep(std::time::Duration::from_secs(300));
            }
        })
        .expect("apps thread");
    list
}

pub fn enumerate() -> Option<Vec<App>> {
    unsafe {
        let folder: IShellItem = SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None).ok()?;
        let items: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems).ok()?;
        let mut apps = Vec::new();
        loop {
            let mut slot = [None];
            let mut fetched = 0;
            if items.Next(&mut slot, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(item) = slot[0].take() else { break };
            let Some(name) = display_name(&item, SIGDN_NORMALDISPLAY) else { continue };
            let Some(id) = display_name(&item, SIGDN_PARENTRELATIVEPARSING) else { continue };
            if is_noise(&name) {
                continue;
            }
            let mut app = App { name, id, path: link_target(&item), lower: String::new(), alias: String::new() };
            app.prepare();
            apps.push(app);
        }
        apps.sort_by(|a, b| a.lower.cmp(&b.lower));
        apps.dedup_by(|a, b| a.lower == b.lower && a.path == b.path);
        Some(apps)
    }
}

/// Uninstallers and readmes clutter the list and are a bad thing to launch by accident.
fn is_noise(name: &str) -> bool {
    let lower = matching::lower(name);
    ["uninstall", "deinstall", "readme", "read me"].iter().any(|n| lower.contains(n)) // i18n-ignore: words in shortcut names to leave out
}

unsafe fn display_name(item: &IShellItem, kind: SIGDN) -> Option<String> {
    unsafe {
        let raw = item.GetDisplayName(kind).ok()?;
        let text = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0 as *const c_void));
        text.filter(|t| !t.is_empty())
    }
}

unsafe fn link_target(item: &IShellItem) -> String {
    unsafe {
        let Ok(item2) = item.cast::<IShellItem2>() else { return String::new() };
        let Ok(raw) = item2.GetString(&PKEY_LINK_TARGET) else { return String::new() };
        let text = raw.to_string().unwrap_or_default();
        CoTaskMemFree(Some(raw.0 as *const c_void));
        // Skip guid-style virtual paths: they can't be shown in a folder.
        if text.contains(":\\") { text } else { String::new() }
    }
}

/// System.Link.TargetParsingPath.
const PKEY_LINK_TARGET: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xb9b4b3fc_2b51_4a42_b5d8_324146afcf25),
    pid: 2,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_apps_are_found_by_their_english_name() {
        assert_eq!(package_name("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App"), "Notepad");
        assert_eq!(package_name("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"), "Calculator");
        assert_eq!(package_name("Microsoft.Paint_8wekyb3d8bbwe!App"), "Paint");
    }

    #[test]
    fn classic_programs_have_no_package_name() {
        assert_eq!(package_name("Chrome"), "");
        assert_eq!(package_name("Microsoft.Windows.Explorer"), "");
        assert_eq!(package_name("{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\taskmgr.exe"), "");
    }
}
