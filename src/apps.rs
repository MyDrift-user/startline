//! The list of installed apps, the same one Windows uses for its own "All apps" list,
//! so both classic programs and Store apps show up.

use crate::matching;
use crate::platform;
use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
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

/// Starts with the saved list so the menu is complete at once, then keeps it up to date as apps come and go.
pub fn start(ctx: egui::Context) -> Arc<RwLock<Vec<App>>> {
    let mut cached: Vec<App> = std::fs::read(cache_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    cached.iter_mut().for_each(App::prepare);
    let list = Arc::new(RwLock::new(cached));
    let (tx, rx) = mpsc::channel();
    watch_packages(tx.clone());
    keep_fresh(list.clone(), programs_folders(), tx, rx, enumerate, move |fresh: &[App]| {
        if let Ok(bytes) = serde_json::to_vec(fresh) {
            let _ = std::fs::write(cache_path(), bytes);
        }
        ctx.request_repaint();
    });
    list
}

/// The Start menu's Programs folders, for everyone on the PC and for this account, where installers put shortcuts.
fn programs_folders() -> Vec<String> {
    [("ProgramData", "Microsoft\\Windows\\Start Menu\\Programs"), ("APPDATA", "Microsoft\\Windows\\Start Menu\\Programs")]
        .iter()
        .filter_map(|(var, rest)| std::env::var(var).ok().map(|base| format!("{base}\\{rest}")))
        .collect()
}

/// After a change, the list is read again this soon, and a few more times a little later, as Windows's own app list
/// takes about three seconds to catch up with a new or removed shortcut.
const AFTER_CHANGE: [Duration; 5] = [Duration::from_millis(500), Duration::from_millis(1500), Duration::from_millis(3000), Duration::from_millis(4500), Duration::from_secs(15)];
/// Read again now and then anyway, in case a change went unseen.
const ANYWAY: Duration = Duration::from_secs(300);

/// Reads the list now, again shortly after anything changes in `folders` or a message comes on `rx`, and every few
/// minutes anyway. `read` gives the list, `done` hears about each new one. Watching runs whether or not files and
/// folders are indexed.
fn keep_fresh(
    list: Arc<RwLock<Vec<App>>>,
    folders: Vec<String>,
    tx: Sender<()>,
    rx: Receiver<()>,
    read: impl Fn() -> Option<Vec<App>> + Send + 'static,
    done: impl Fn(&[App]) + Send + 'static,
) {
    std::thread::Builder::new()
        .name("apps".into())
        .spawn(move || {
            platform::com_init();
            // Kept for as long as the thread runs.
            let _watchers: Vec<platform::DirWatcher> = folders
                .iter()
                .filter_map(|folder| {
                    let tx = tx.clone();
                    platform::DirWatcher::start(folder, move |_| {
                        let _ = tx.send(());
                    })
                })
                .collect();
            let mut again: Vec<Instant> = vec![Instant::now()];
            loop {
                let next = again.iter().min().copied().unwrap_or_else(|| Instant::now() + ANYWAY);
                match rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
                    Ok(()) => {
                        // A burst of changes, as while an installer runs, counts from its last change.
                        let now = Instant::now();
                        again = AFTER_CHANGE.iter().map(|d| now + *d).collect();
                        continue;
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
                let now = Instant::now();
                again.retain(|at| *at > now);
                if let Some(fresh) = read() {
                    let changed = list.read().map(|current| *current != fresh).unwrap_or(true);
                    if changed {
                        if let Ok(mut current) = list.write() {
                            *current = fresh.clone();
                        }
                        done(&fresh);
                    }
                }
            }
        })
        .expect("apps thread");
}

/// Store and other packaged apps don't put shortcuts in the Programs folders: Windows says when one is installed,
/// updated or removed.
fn watch_packages(tx: Sender<()>) {
    use windows::ApplicationModel::*;
    use windows::Foundation::TypedEventHandler;
    let _ = std::thread::Builder::new().name("app-packages".into()).spawn(move || {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let Ok(catalog) = PackageCatalog::OpenForCurrentUser() else { return };
        let ping = |tx: &Sender<()>| {
            let _ = tx.send(());
        };
        let (a, b, c, d) = (tx.clone(), tx.clone(), tx.clone(), tx);
        let _ = catalog.PackageInstalling(&TypedEventHandler::new(move |_, args: Ref<PackageInstallingEventArgs>| {
            if args.as_ref().is_some_and(|e| e.IsComplete().unwrap_or(true)) {
                ping(&a);
            }
            Ok(())
        }));
        let _ = catalog.PackageUninstalling(&TypedEventHandler::new(move |_, args: Ref<PackageUninstallingEventArgs>| {
            if args.as_ref().is_some_and(|e| e.IsComplete().unwrap_or(true)) {
                ping(&b);
            }
            Ok(())
        }));
        let _ = catalog.PackageUpdating(&TypedEventHandler::new(move |_, args: Ref<PackageUpdatingEventArgs>| {
            if args.as_ref().is_some_and(|e| e.IsComplete().unwrap_or(true)) {
                ping(&c);
            }
            Ok(())
        }));
        let _ = catalog.PackageStatusChanged(&TypedEventHandler::new(move |_, _: Ref<PackageStatusChangedEventArgs>| {
            ping(&d);
            Ok(())
        }));
        // The catalog has to stay alive for its events to keep coming.
        loop {
            std::thread::park();
        }
    });
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
    fn a_shortcut_added_to_a_watched_programs_folder_shows_up_without_a_restart() {
        let folder = std::env::temp_dir().join(format!("startline-programs-{}", std::process::id()));
        std::fs::create_dir_all(folder.join("Tools")).unwrap();
        let dir = folder.to_string_lossy().to_string();
        // Stands in for Windows's app list: every shortcut in the folder and the ones inside it.
        let shortcuts = dir.clone();
        let read = move || {
            let mut apps = Vec::new();
            for sub in ["", "Tools"] {
                for entry in std::fs::read_dir(std::path::Path::new(&shortcuts).join(sub)).ok()?.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if let Some(stem) = name.strip_suffix(".lnk") {
                        let mut app = App { name: stem.into(), id: name.clone(), path: String::new(), lower: String::new(), alias: String::new() };
                        app.prepare();
                        apps.push(app);
                    }
                }
            }
            apps.sort_by(|a, b| a.lower.cmp(&b.lower));
            Some(apps)
        };
        let list = Arc::new(RwLock::new(Vec::new()));
        let (tx, rx) = mpsc::channel();
        keep_fresh(list.clone(), vec![dir.clone()], tx, rx, read, |_| {});
        let names = || list.read().unwrap().iter().map(|a| a.name.clone()).collect::<Vec<_>>();
        let wait_for = |want: &dyn Fn(&[String]) -> bool| {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(5) {
                if want(&names()) {
                    return started.elapsed();
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            panic!("the list is still {:?}", names());
        };
        std::thread::sleep(Duration::from_millis(300));

        std::fs::write(folder.join("CaptureCraft.lnk"), b"").unwrap();
        let took = wait_for(&|n| n.iter().any(|a| a == "CaptureCraft"));
        assert!(took < Duration::from_secs(2), "it should take a second or two, not {took:?}");
        std::fs::write(folder.join("Tools").join("Helper.lnk"), b"").unwrap();
        wait_for(&|n| n.iter().any(|a| a == "Helper"));
        std::fs::rename(folder.join("CaptureCraft.lnk"), folder.join("CaptureCraft 2.lnk")).unwrap();
        wait_for(&|n| n.iter().any(|a| a == "CaptureCraft 2") && !n.iter().any(|a| a == "CaptureCraft"));
        std::fs::remove_file(folder.join("CaptureCraft 2.lnk")).unwrap();
        wait_for(&|n| !n.iter().any(|a| a.starts_with("CaptureCraft")));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn classic_programs_have_no_package_name() {
        assert_eq!(package_name("Chrome"), "");
        assert_eq!(package_name("Microsoft.Windows.Explorer"), "");
        assert_eq!(package_name("{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\taskmgr.exe"), "");
    }
}
