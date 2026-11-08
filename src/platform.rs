//! Small wrappers around Windows: reading folders, watching for changes, opening things.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::IO::CancelIoEx;
use windows::Win32::System::Threading::*;
use windows::Win32::NetworkManagement::WNet::WNetGetConnectionW;
use windows::Win32::System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOTE, DRIVE_REMOVABLE};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

pub fn wide(text: &str) -> Vec<u16> {
    std::ffi::OsStr::new(text).encode_wide().chain(Some(0)).collect()
}

pub fn unix_now() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0)
}

fn filetime_to_unix(time: FILETIME) -> u32 {
    let ticks = ((time.dwHighDateTime as u64) << 32) | time.dwLowDateTime as u64;
    (ticks / 10_000_000).saturating_sub(11_644_473_600).min(u32::MAX as u64) as u32
}

fn long_path(path: &str) -> String {
    if path.starts_with("\\\\") { path.to_string() } else { format!("\\\\?\\{path}") }
}

/// Calls `visit(name, is_dir, is_link, modified)` for each item in a folder.
/// Attributes that can leave an item out of a listing.
pub const HIDDEN: u32 = FILE_ATTRIBUTE_HIDDEN.0;
pub const SYSTEM: u32 = FILE_ATTRIBUTE_SYSTEM.0;
pub const HIDDEN_OR_SYSTEM: u32 = HIDDEN | SYSTEM;

/// Calls `visit` with each item's name, whether it's a folder, whether it's a link, and when it last changed.
/// Items with any of the `skip_attrs` attributes are left out.
pub fn list_dir(path: &str, skip_attrs: u32, mut visit: impl FnMut(&str, bool, bool, u32)) {
    let pattern = wide(&format!("{}\\*", long_path(path.trim_end_matches('\\'))));
    let mut data = WIN32_FIND_DATAW::default();
    let handle = unsafe {
        FindFirstFileExW(
            PCWSTR(pattern.as_ptr()),
            FindExInfoBasic,
            &mut data as *mut _ as *mut c_void,
            FindExSearchNameMatch,
            None,
            FIND_FIRST_EX_LARGE_FETCH,
        )
    };
    let Ok(handle) = handle else { return };
    let mut name_buf = String::new();
    loop {
        let raw = &data.cFileName;
        let len = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
        let name = &raw[..len];
        let is_nav = name == [b'.' as u16] || name == [b'.' as u16, b'.' as u16];
        let attrs = data.dwFileAttributes;
        let skip = attrs & skip_attrs != 0;
        if !is_nav && !skip {
            name_buf.clear();
            name_buf.extend(std::char::decode_utf16(name.iter().copied()).map(|c| c.unwrap_or('\u{FFFD}')));
            visit(
                &name_buf,
                attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0,
                attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0,
                filetime_to_unix(data.ftLastWriteTime),
            );
        }
        if unsafe { FindNextFileW(handle, &mut data) }.is_err() {
            break;
        }
    }
    let _ = unsafe { FindClose(handle) };
}

fn attributes(path: &str) -> Option<WIN32_FILE_ATTRIBUTE_DATA> {
    let wide_path = wide(&long_path(path));
    let mut data = WIN32_FILE_ATTRIBUTE_DATA::default();
    unsafe { GetFileAttributesExW(PCWSTR(wide_path.as_ptr()), GetFileExInfoStandard, &mut data as *mut _ as *mut c_void) }
        .ok()
        .map(|_| data)
}

/// `(is_dir, modified)` for a path, or `None` if it no longer exists.
pub fn stat(path: &str) -> Option<(bool, u32)> {
    let data = attributes(path)?;
    Some((data.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0, filetime_to_unix(data.ftLastWriteTime)))
}

/// Whether the item has any of these attributes, such as [`HIDDEN`] or [`SYSTEM`].
pub fn has_attributes(path: &str, mask: u32) -> bool {
    attributes(path).is_some_and(|d| d.dwFileAttributes & mask != 0)
}

pub fn lower_thread_priority() {
    let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
}

/// Fixed drives other than the Windows one.
pub fn other_fixed_drives() -> Vec<String> {
    let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).to_uppercase();
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| format!("{}:\\", (b'A' + i) as char))
        .filter(|root| {
            let w = wide(root);
            !root.starts_with(&system) && unsafe { GetDriveTypeW(PCWSTR(w.as_ptr())) } == DRIVE_FIXED
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DriveKind {
    Fixed,
    Network,
    Removable,
}

#[derive(Clone, Debug)]
pub struct Drive {
    /// "Z:\".
    pub root: String,
    pub kind: DriveKind,
    /// The volume's name, or where a network drive points.
    pub label: String,
}

/// Which drives are attached and of what kind. Quick: it doesn't touch the drives themselves.
pub fn attached_drives() -> Vec<(String, DriveKind)> {
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .filter_map(|i| {
            let root = format!("{}:\\", (b'A' + i) as char);
            let w = wide(&root);
            let kind = match unsafe { GetDriveTypeW(PCWSTR(w.as_ptr())) } {
                DRIVE_FIXED => DriveKind::Fixed,
                DRIVE_REMOTE => DriveKind::Network,
                DRIVE_REMOVABLE => DriveKind::Removable,
                _ => return None,
            };
            Some((root, kind))
        })
        .collect()
}

/// The attached drives with their names. A network drive can be slow to answer, so call
/// this from a helper thread.
pub fn describe_drives() -> Vec<Drive> {
    attached_drives()
        .into_iter()
        .map(|(root, kind)| {
            let w = wide(&root);
            let label = if kind == DriveKind::Network {
                let letter = wide(root.trim_end_matches('\\'));
                let mut buffer = [0u16; 512];
                let mut size = buffer.len() as u32;
                let status = unsafe { WNetGetConnectionW(PCWSTR(letter.as_ptr()), Some(PWSTR(buffer.as_mut_ptr())), &mut size) };
                let len = buffer.iter().position(|&c| c == 0).unwrap_or(0);
                if status == NO_ERROR { String::from_utf16_lossy(&buffer[..len]) } else { String::new() }
            } else {
                let mut buffer = [0u16; 128];
                let _ = unsafe { GetVolumeInformationW(PCWSTR(w.as_ptr()), Some(&mut buffer), None, None, None, None) };
                let len = buffer.iter().position(|&c| c == 0).unwrap_or(0);
                String::from_utf16_lossy(&buffer[..len])
            };
            Drive { root, kind, label }
        })
        .collect()
}

/// The folders directly inside a folder, as (name, its HIDDEN and SYSTEM attributes), plain ones first.
pub fn child_folders(path: &str) -> Vec<(String, u32)> {
    let folders = |skip: u32| {
        let mut names = Vec::new();
        list_dir(path, skip, |name, is_dir, is_link, _| {
            if is_dir && !is_link {
                names.push(name.to_string());
            }
        });
        names
    };
    let (not_hidden, not_system) = (folders(HIDDEN), folders(SYSTEM));
    let mut all: Vec<(String, u32)> = folders(0)
        .into_iter()
        .map(|name| {
            let hidden = if not_hidden.contains(&name) { 0 } else { HIDDEN };
            let system = if not_system.contains(&name) { 0 } else { SYSTEM };
            (name, hidden | system)
        })
        .collect();
    all.sort_by_key(|(name, attrs)| (*attrs != 0, name.to_lowercase()));
    all
}

// ---- Watching folders ----

pub enum DirEvent {
    Added(String),
    Removed(String),
    Modified(String),
    Overflow,
}

pub struct DirWatcher {
    stop: Arc<AtomicBool>,
    handle: usize,
}

impl DirWatcher {
    pub fn start(path: &str, on_event: impl Fn(DirEvent) + Send + 'static) -> Option<DirWatcher> {
        let wide_path = wide(path);
        let handle = unsafe {
            CreateFileW(
                PCWSTR(wide_path.as_ptr()),
                FILE_LIST_DIRECTORY.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
        }
        .ok()?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();
        let raw = handle.0 as usize;
        std::thread::Builder::new()
            .name("watch".into())
            .spawn(move || {
                let handle = HANDLE(raw as *mut c_void);
                // Aligned to 4 bytes, as Windows requires.
                let mut buffer = vec![0u32; 32 * 1024];
                let filter = FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE;
                while !stop_flag.load(Ordering::Relaxed) {
                    let mut returned = 0u32;
                    let ok = unsafe {
                        ReadDirectoryChangesW(
                            handle,
                            buffer.as_mut_ptr() as *mut c_void,
                            (buffer.len() * 4) as u32,
                            true,
                            filter,
                            Some(&mut returned),
                            None,
                            None,
                        )
                    };
                    if ok.is_err() {
                        break;
                    }
                    if returned == 0 {
                        on_event(DirEvent::Overflow);
                        continue;
                    }
                    let mut at = 0usize;
                    let base = buffer.as_ptr() as *const u8;
                    loop {
                        let info = unsafe { &*(base.add(at) as *const FILE_NOTIFY_INFORMATION) };
                        let name_ptr = std::ptr::addr_of!(info.FileName) as *const u16;
                        let units = unsafe { std::slice::from_raw_parts(name_ptr, info.FileNameLength as usize / 2) };
                        let name = String::from_utf16_lossy(units);
                        on_event(match info.Action {
                            FILE_ACTION_ADDED | FILE_ACTION_RENAMED_NEW_NAME => DirEvent::Added(name),
                            FILE_ACTION_REMOVED | FILE_ACTION_RENAMED_OLD_NAME => DirEvent::Removed(name),
                            _ => DirEvent::Modified(name),
                        });
                        if info.NextEntryOffset == 0 {
                            break;
                        }
                        at += info.NextEntryOffset as usize;
                    }
                }
                let _ = unsafe { CloseHandle(handle) };
            })
            .ok()?;
        Some(DirWatcher { stop, handle: raw })
    }
}

impl Drop for DirWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = unsafe { CancelIoEx(HANDLE(self.handle as *mut c_void), None) };
    }
}

// ---- Opening things ----

pub fn com_init() {
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
}

/// Whether this copy of Startline runs with administrator rights, as when it was started from an elevated prompt.
pub fn is_elevated() -> bool {
    use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut size = 0;
        let ok = GetTokenInformation(token, TokenElevation, Some(&mut elevation as *mut _ as *mut c_void), size_of::<TOKEN_ELEVATION>() as u32, &mut size).is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

/// Opens through the desktop's own Explorer, so what opens runs with the person's normal rights even when Startline
/// has administrator rights. False if there's no desktop Explorer to ask.
fn shell_open_as_user(file: &str, args: &str) -> bool {
    use windows::Win32::System::Com::{CLSCTX_LOCAL_SERVER, CoCreateInstance, IDispatch, IServiceProvider};
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::UI::Shell::*;
    use windows::core::{BSTR, Interface};
    unsafe {
        let run = || -> windows::core::Result<()> {
            let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)?;
            let desktop = VARIANT::from(CSIDL_DESKTOP as i32);
            let mut hwnd = 0;
            let found: IDispatch = windows.FindWindowSW(&desktop, &VARIANT::default(), SWC_DESKTOP, &mut hwnd, SWFO_NEEDDISPATCH)?;
            let browser: IShellBrowser = found.cast::<IServiceProvider>()?.QueryService(&SID_STopLevelBrowser)?;
            let view = browser.QueryActiveShellView()?;
            let background: IDispatch = view.GetItemObject(SVGIO_BACKGROUND)?;
            let shell: IShellDispatch2 = background.cast::<IShellFolderViewDual>()?.Application()?.cast()?;
            let show = VARIANT::from(SW_SHOWNORMAL.0);
            shell.ShellExecute(&BSTR::from(file), &VARIANT::from(BSTR::from(args)), &VARIANT::from(BSTR::new()), &VARIANT::from(BSTR::new()), &show)
        };
        run().is_ok()
    }
}

pub fn shell_open(verb: &str, file: &str, args: &str) -> bool {
    // An elevated Start menu mustn't make everything it opens elevated too; only Run as administrator should.
    if verb != "runas" && is_elevated() && shell_open_as_user(file, args) {
        return true;
    }
    let (verb_w, file_w, args_w) = (wide(verb), wide(file), wide(args));
    let result = unsafe {
        ShellExecuteW(
            None,
            if verb.is_empty() { PCWSTR::null() } else { PCWSTR(verb_w.as_ptr()) },
            PCWSTR(file_w.as_ptr()),
            if args.is_empty() { PCWSTR::null() } else { PCWSTR(args_w.as_ptr()) },
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    result.0 as usize > 32
}

/// Opens in the background so a slow program can never freeze the menu.
pub fn open_async(verb: &'static str, file: String, args: String) {
    std::thread::spawn(move || {
        com_init();
        shell_open(verb, &file, &args);
    });
}

pub fn open_target(target: &crate::config::Target, admin: bool) {
    let verb = if admin { "runas" } else { "" };
    match target {
        crate::config::Target::App(id) => open_async(verb, format!("shell:AppsFolder\\{id}"), String::new()),
        crate::config::Target::Path { path, args } => open_async(verb, path.clone(), args.clone()),
    }
}

/// Opens the folder with this file selected.
pub fn show_in_folder(path: &str) {
    open_async("", "explorer.exe".into(), format!("/select,\"{path}\""));
}

pub fn copy_text(text: &str) -> std::result::Result<(), ()> {
    use windows::Win32::System::DataExchange::*;
    use windows::Win32::System::Memory::*;
    const CF_UNICODETEXT: u32 = 13;
    let units = wide(text);
    unsafe {
        OpenClipboard(None).map_err(|_| ())?;
        let _ = EmptyClipboard();
        let result = (|| -> std::result::Result<(), ()> {
            let mem = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2).map_err(|_| ())?;
            let ptr = GlobalLock(mem) as *mut u16;
            if ptr.is_null() {
                return Err(());
            }
            std::ptr::copy_nonoverlapping(units.as_ptr(), ptr, units.len());
            let _ = GlobalUnlock(mem);
            SetClipboardData(CF_UNICODETEXT, Some(HANDLE(mem.0))).map_err(|_| ())?;
            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
}

/// Files the person opened recently, newest first, taken from the Windows "Recent" folder.
pub fn recent_files(limit: usize) -> Vec<(String, String, u32)> {
    let Some(appdata) = std::env::var_os("APPDATA") else { return Vec::new() };
    let folder = std::path::PathBuf::from(appdata).join("Microsoft\\Windows\\Recent");
    let mut links: Vec<(String, u32)> = Vec::new();
    list_dir(&folder.to_string_lossy(), HIDDEN_OR_SYSTEM, |name, is_dir, _, mtime| {
        if !is_dir && name.to_ascii_lowercase().ends_with(".lnk") {
            links.push((name.to_string(), mtime));
        }
    });
    links.sort_unstable_by_key(|l| std::cmp::Reverse(l.1));

    let mut out: Vec<(String, String, u32)> = Vec::new();
    for (name, mtime) in links.into_iter().take(limit * 6) {
        if out.len() >= limit {
            break;
        }
        let link = folder.join(&name);
        let Some(target) = resolve_link(&link.to_string_lossy()) else { continue };
        // Folders land in Recent too; this list is about files.
        if let Some((false, _)) = stat(&target) {
            let display = target.rsplit('\\').next().unwrap_or(&target).to_string();
            if !out.iter().any(|o| o.1 == target) {
                out.push((display, target, mtime));
            }
        }
    }
    out
}

fn resolve_link(path: &str) -> Option<String> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        let w = wide(path);
        file.Load(PCWSTR(w.as_ptr()), STGM_READ).ok()?;
        let mut buffer = [0u16; 1024];
        link.GetPath(&mut buffer, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32).ok()?;
        let len = buffer.iter().position(|&c| c == 0)?;
        (len > 0).then(|| String::from_utf16_lossy(&buffer[..len]))
    }
}

pub fn user_name() -> String {
    use std::sync::OnceLock;
    use windows::Win32::Security::Authentication::Identity::{GetUserNameExW, NameDisplay};
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        // The name you chose for your account, where there is one, rather than the login.
        let mut buffer = [0u16; 256];
        let mut size = buffer.len() as u32;
        let found = unsafe { GetUserNameExW(NameDisplay, Some(PWSTR(buffer.as_mut_ptr())), &mut size) };
        if found && size > 0 {
            return String::from_utf16_lossy(&buffer[..size as usize]);
        }
        std::env::var("USERNAME").unwrap_or_else(|_| "You".into())
    })
    .clone()
}

/// Where Windows keeps this account's picture, or its stand-in if none was ever set.
pub fn account_picture() -> Option<String> {
    // Windows' own lookup, which also unpacks a picture stored inside the system.
    type Lookup = unsafe extern "system" fn(PCWSTR, u32, PWSTR, u32) -> HRESULT;
    const CREATE_PICTURE_FILE: u32 = 0x8000_0000;
    let looked_up = unsafe {
        use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        LoadLibraryW(w!("shell32.dll")).ok().and_then(|shell| GetProcAddress(shell, PCSTR(261usize as *const u8))).and_then(|raw| {
            let lookup: Lookup = std::mem::transmute(raw);
            let mut buffer = [0u16; 520];
            let status = lookup(PCWSTR::null(), CREATE_PICTURE_FILE, PWSTR(buffer.as_mut_ptr()), buffer.len() as u32);
            let len = buffer.iter().position(|&c| c == 0).unwrap_or(0);
            (status.is_ok() && len > 0).then(|| String::from_utf16_lossy(&buffer[..len]))
        })
    };
    if let Some(path) = looked_up.filter(|p| stat(p).is_some()) {
        return Some(path);
    }
    let common = std::env::var("ProgramData").ok()?;
    let fallback = format!("{common}\\Microsoft\\User Account Pictures\\user-192.png");
    stat(&fallback).map(|_| fallback)
}

/// Asks Windows to do something to a file, through the same doors as its own menus.
fn shell_verb(verb: &str, file: &str) {
    com_init();
    let (verb_w, file_w) = (wide(verb), wide(file));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_INVOKEIDLIST,
        lpVerb: PCWSTR(verb_w.as_ptr()),
        lpFile: PCWSTR(file_w.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    let _ = unsafe { ShellExecuteExW(&mut info) };
    // Properties opens inside this thread and closes with it, so give it time to be used.
    if verb == "properties" {
        std::thread::sleep(std::time::Duration::from_secs(600));
    }
}

thread_local! {
    /// The Windows menu on screen, which draws some of its own items.
    static SHELL_MENU: std::cell::RefCell<Option<IContextMenu2>> = const { std::cell::RefCell::new(None) };
}

unsafe extern "system" fn shell_menu_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if matches!(message, WM_INITMENUPOPUP | WM_DRAWITEM | WM_MEASUREITEM) {
        SHELL_MENU.with(|menu| {
            if let Some(menu) = menu.borrow().as_ref() {
                let _ = unsafe { menu.HandleMenuMsg(message, wparam, lparam) };
            }
        });
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

/// Shows Windows' own right-click menu for something, at the pointer, and does what's picked.
/// Returns once it's closed, so call it from a helper thread.
pub fn shell_context_menu(source: &str) -> std::result::Result<(), String> {
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    com_init();
    unsafe {
        let source_w = wide(source);
        let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(source_w.as_ptr()), None).map_err(|e| format!("finding it: {e}"))?;
        let menu: IContextMenu = item.BindToHandler(None, &BHID_SFUIObject).map_err(|e| format!("asking for its menu: {e}"))?;
        let instance = GetModuleHandleW(None).map(|m| HINSTANCE(m.0)).unwrap_or_default();
        let class = w!("StartlineShellMenu");
        let info = WNDCLASSW { lpfnWndProc: Some(shell_menu_proc), hInstance: instance, lpszClassName: class, ..Default::default() };
        RegisterClassW(&info);
        let window = CreateWindowExW(Default::default(), class, PCWSTR::null(), WS_POPUP, 0, 0, 0, 0, None, None, Some(instance), None).map_err(|e| format!("making its window: {e}"))?;
        let popup = CreatePopupMenu().map_err(|e| format!("making the menu: {e}"))?;
        let filled = menu.QueryContextMenu(popup, 0, 1, 0x7FFF, CMF_NORMAL);
        if filled.is_err() {
            return Err(format!("filling the menu: {filled:?}"));
        }
        SHELL_MENU.with(|slot| *slot.borrow_mut() = menu.cast().ok());
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let _ = SetForegroundWindow(window);
        let picked = TrackPopupMenuEx(popup, (TPM_RETURNCMD | TPM_RIGHTBUTTON).0, point.x, point.y, window, None).0;
        if picked > 0 {
            let command = CMINVOKECOMMANDINFO {
                cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
                hwnd: window,
                lpVerb: PCSTR((picked - 1) as usize as *const u8),
                nShow: SW_SHOWNORMAL.0,
                ..Default::default()
            };
            let _ = menu.InvokeCommand(&command);
        }
        SHELL_MENU.with(|slot| *slot.borrow_mut() = None);
        let _ = DestroyMenu(popup);
        let _ = DestroyWindow(window);
    }
    Ok(())
}

/// Makes a normal window's title bar match the dark or light look.
pub fn set_title_bar_dark(title: &str, dark: bool) -> bool {
    use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
    let title_w = wide(title);
    unsafe {
        let Ok(window) = FindWindowW(PCWSTR::null(), PCWSTR(title_w.as_ptr())) else { return false };
        let value = BOOL::from(dark);
        DwmSetWindowAttribute(window, DWMWA_USE_IMMERSIVE_DARK_MODE, &value as *const _ as *const c_void, std::mem::size_of::<BOOL>() as u32).is_ok()
    }
}

pub fn show_properties(path: String) {
    std::thread::spawn(move || shell_verb("properties", &path));
}

pub fn open_with(path: String) {
    std::thread::spawn(move || shell_verb("openas", &path));
}

/// Sends to the Recycle Bin, with Windows' own confirmation if that's how it's set up.
pub fn recycle(path: String) {
    std::thread::spawn(move || {
        com_init();
        let mut from = wide(&path);
        from.push(0);
        let mut operation = SHFILEOPSTRUCTW { wFunc: FO_DELETE, pFrom: PCWSTR(from.as_ptr()), fFlags: FOF_ALLOWUNDO.0 as u16, ..Default::default() };
        let _ = unsafe { SHFileOperationW(&mut operation) };
    });
}

/// Opens the native picker for a folder or a file, in front of `owner`. Blocks, so call it
/// from a helper thread.
pub fn pick_path(owner: isize, folder: bool) -> Option<String> {
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let options = if folder { FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM } else { FOS_FORCEFILESYSTEM };
        dialog.SetOptions(options).ok()?;
        let parent = (owner != 0).then_some(HWND(owner as *mut c_void));
        dialog.Show(parent).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let text = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as *const c_void));
        text
    }
}

pub fn message_box_error(text: &str) {
    let (text_w, title) = (wide(text), wide("Startline"));
    unsafe {
        MessageBoxW(None, PCWSTR(text_w.as_ptr()), PCWSTR(title.as_ptr()), MB_OK | MB_ICONERROR);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run from an elevated prompt: cargo test --release -- --ignored opens_with_normal_rights
    #[test]
    #[ignore = "needs an elevated prompt and a desktop"]
    fn opens_with_normal_rights_even_when_elevated() {
        assert!(is_elevated(), "start this test from an elevated prompt");
        com_init();
        let out = std::env::temp_dir().join(format!("startline-rights-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&out);
        assert!(shell_open("", "cmd.exe", &format!("/c whoami /groups > \"{}\"", out.display())));
        let mut groups = String::new();
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(200));
            // whoami writes in the console's code page, not UTF-8
            groups = String::from_utf8_lossy(&std::fs::read(&out).unwrap_or_default()).into_owned();
            if groups.contains("S-1-16-") {
                break;
            }
        }
        let _ = std::fs::remove_file(&out);
        assert!(groups.contains("S-1-16-8192"), "it should run at the normal (medium) level");
        assert!(!groups.contains("S-1-16-12288"), "not with administrator rights");
    }
}
