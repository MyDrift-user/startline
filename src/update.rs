//! Updates from GitHub releases. Every release carries `latest.json`, listing its version and the SHA-256 of each file,
//! and `latest.json.sig`, an Ed25519 signature of that list. Nothing is installed unless the signature matches a key
//! built in here and the file matches its hash.
//!
//! Startline is always running, so an update can't wait for it to close. Instead it is closed, updated and started
//! again, at a moment the menu isn't on screen:
//!
//! - Installed for one person: a copy of this program under another name runs the installer, which closes Startline,
//!   and then starts the new version.
//! - Installed for the whole PC: a scheduled task runs `--update-service` as SYSTEM, does the same, and starts the new
//!   version again in every session that had it running. Nobody sees an admin prompt.
//! - Portable: the program replaces its own file and starts the new one.

use crate::config::data_dir;
use crate::i18n::tr;
use crate::policy::{self, Policies};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
pub const PROJECT: &str = "https://github.com/MyDrift-user/startline";
pub const RELEASES: &str = "https://github.com/MyDrift-user/startline/releases";
const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);
/// Startline starts with Windows: a check shouldn't compete with signing in.
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(90);
/// The name the copy that runs the installer goes by, so the installer's "close Startline" leaves it alone.
const HELPER: &str = "startline-update-helper.exe";
/// The public half of the key that signs releases. The private half is a GitHub secret.
#[cfg_attr(feature = "update-test", allow(dead_code))]
const PUBLIC_KEY: [u8; 32] = [0xde, 0x3e, 0x29, 0x41, 0x84, 0xe2, 0xd0, 0x1a, 0xb7, 0x73, 0xfc, 0x17, 0x89, 0xd5, 0xfe, 0xde, 0x9a, 0xb7, 0xbd, 0x49, 0x78, 0x89, 0x00, 0x82, 0x13, 0xfc, 0x79, 0x67, 0x1a, 0x27, 0x7c, 0x08];
/// A second key, kept offline and never used, so a leaked main key can be replaced: a release signed with this one
/// can still reach every copy, and leave the leaked key out from then on.
#[cfg_attr(feature = "update-test", allow(dead_code))]
const BACKUP_PUBLIC_KEY: [u8; 32] = [0x3c, 0x21, 0x30, 0x91, 0x72, 0xc3, 0x29, 0x6a, 0x15, 0x2d, 0xfc, 0x8b, 0x09, 0x80, 0x90, 0x13, 0x9e, 0xbc, 0x82, 0x04, 0xc5, 0x6b, 0x31, 0x3e, 0xa9, 0x32, 0xf6, 0xbd, 0xe9, 0x59, 0xd3, 0xcd];

/// The `update-test` feature is only for the release workflow's update test, which must not depend on the real key.
/// A program built with it trusts this published key, and is never released: the release job checks that the program
/// it publishes accepts the real signature.
#[cfg(feature = "update-test")]
const TEST_SEED: [u8; 32] = [7; 32];

#[cfg(feature = "update-test")]
fn trusted_keys() -> Vec<[u8; 32]> {
    vec![ed25519_dalek::SigningKey::from_bytes(&TEST_SEED).verifying_key().to_bytes()]
}

#[cfg(not(feature = "update-test"))]
fn trusted_keys() -> Vec<[u8; 32]> {
    vec![PUBLIC_KEY, BACKUP_PUBLIC_KEY]
}

// ---- A log, as nobody watches the update task ----

fn log_path() -> PathBuf {
    data_dir().join("update.log")
}

pub fn log(line: &str) {
    use std::io::Write;
    let path = log_path();
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 512 * 1024) {
        let _ = std::fs::rename(&path, path.with_extension("old.log"));
    }
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{:04}-{:02}-{:02} {:02}:{:02}:{:02} [{}] {line}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, std::process::id());
    }
}

// ---- Versions and the signed list ----

/// (major, minor, patch), so they compare as numbers. None for anything else.
fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let mut parts = v.trim().trim_start_matches('v').split('.');
    let t = (parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?);
    parts.next().is_none().then_some(t)
}

fn is_newer(candidate: &str, than: &str) -> bool {
    matches!((parse_version(candidate), parse_version(than)), (Some(a), Some(b)) if a > b)
}

#[derive(Debug, PartialEq)]
struct Manifest {
    version: String,
    /// (file name, SHA-256 in lowercase hex)
    files: Vec<(String, String)>,
}

/// Lines of `version 1.2.3` and `file <sha256> <name>`. Only called on a list whose signature checked out.
fn parse_manifest(text: &str) -> Option<Manifest> {
    let mut version = None;
    let mut files = Vec::new();
    for line in text.lines() {
        match line.split_once(' ') {
            Some(("version", v)) => version = Some(v.trim().to_string()),
            Some(("file", rest)) => {
                let (hash, name) = rest.split_once(' ')?;
                files.push((name.trim().to_string(), hash.trim().to_ascii_lowercase()));
            }
            _ => {}
        }
    }
    let version = version.filter(|v| parse_version(v).is_some())?;
    Some(Manifest { version, files })
}

fn signature_ok(key: &[u8; 32], text: &[u8], sig: &[u8]) -> bool {
    let (Ok(key), Ok(sig)) = (VerifyingKey::from_bytes(key), <[u8; 64]>::try_from(sig)) else { return false };
    key.verify_strict(text, &Signature::from_bytes(&sig)).is_ok()
}

fn verified_manifest(text: &[u8], sig: &[u8]) -> Result<Manifest, String> {
    if !trusted_keys().iter().any(|key| signature_ok(key, text, sig)) {
        return Err(tr("The update's signature doesn't match, so it was ignored").into());
    }
    std::str::from_utf8(text).ok().and_then(parse_manifest).ok_or_else(|| tr("The update's file list couldn't be read").to_string())
}

fn sha256_of(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).ok()?;
    Some(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

// ---- What administrators allow ----

/// Whether Startline may look for updates at all.
pub fn allowed(policy: &Policies) -> bool {
    policy.allows(policy::CHECK_FOR_UPDATES)
}

/// Whether the task installs updates on a PC-wide install: an administrator's policy, then the switch.
fn machine_may_update(policy: &Policies, switch: Option<bool>) -> bool {
    if !allowed(policy) {
        return false;
    }
    if let Some(on) = policy.fixed("/auto_update").and_then(serde_json::Value::as_bool) {
        return on;
    }
    switch.unwrap_or(true)
}

/// The settings page's switch for a PC-wide install, as the task can't read anyone's settings: 0 or 1 in this file.
fn machine_switch_file() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("ProgramData")?).join("Startline").join("auto-update.txt")) // i18n-ignore: folder name
}

/// What the switch says, if anyone has used it.
pub fn machine_switch() -> Option<bool> {
    let text = std::fs::read_to_string(machine_switch_file()?).ok()?;
    match text.trim() {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

/// Writing it needs no admin rights, but only the account that first wrote it, or an admin, can change it.
pub fn set_machine_switch(on: bool) -> Result<(), String> {
    let Some(file) = machine_switch_file() else { return Ok(()) };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&file, if on { "1" } else { "0" }).map_err(|_| tr("Another account set this for the whole PC. That account or an administrator can change it").to_string())
}

// ---- How this copy was installed ----

#[derive(Clone, Debug, PartialEq)]
pub enum Install {
    /// A setup.exe or MSI install in a folder this person can write to.
    PerUser { msi: bool },
    /// A setup.exe or MSI install for everyone, which the scheduled task keeps up to date.
    Machine { msi: bool },
    /// One file that can be replaced.
    Portable(PathBuf),
    /// Can't be changed from here, such as a plain program in a read-only folder.
    Unmanaged,
}

fn folder_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".startline-write-test-{}", std::process::id()));
    let ok = std::fs::File::create(&probe).is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

pub fn detect() -> Install {
    let Ok(exe) = std::env::current_exe() else { return Install::Unmanaged };
    let Some(dir) = exe.parent().map(Path::to_path_buf) else { return Install::Unmanaged };
    // the installers add the licence as LICENSE.txt, which the portable program doesn't have next to it
    if dir.join("LICENSE.txt").exists() {
        let msi = !dir.join("unins000.exe").exists();
        // PC-wide installs are marked: being able to write to the folder says nothing, as admins and SYSTEM can
        let machine = dir.join("machine-install.txt").exists() || !folder_writable(&dir);
        return if machine { Install::Machine { msi } } else { Install::PerUser { msi } };
    }
    if folder_writable(&dir) { Install::Portable(exe) } else { Install::Unmanaged }
}

/// The ending of the release file this install needs, after `startline-v1.2.3-`.
fn asset_suffix(install: &Install) -> Option<&'static str> {
    Some(match install {
        Install::PerUser { msi: true } | Install::Machine { msi: true } => "windows-x64.msi",
        Install::PerUser { msi: false } | Install::Machine { msi: false } => "windows-x64-setup.exe",
        Install::Portable(_) => "windows-x64-portable.exe",
        Install::Unmanaged => return None,
    })
}

// ---- Downloading ----

fn hidden(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut c = Command::new(program);
    c.creation_flags(0x0800_0000); // no console window
    c
}

/// The tools come from the system folder by full path, as this also runs as SYSTEM.
fn system_tool(name: &str) -> String {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    format!(r"{root}\System32\{name}.exe")
}

/// Test hook: STARTLINE_TEST_UPDATE_URL points at another server. Everything must still be signed by a trusted key.
fn base_url(service: bool) -> Option<String> {
    // the update task has no environment of its own to set, so a test build reads the address from a file
    #[cfg(feature = "update-test")]
    if let Ok(url) = std::fs::read_to_string(r"C:\startline-update-test.txt") {
        return Some(url.trim().to_string());
    }
    if service {
        return None;
    }
    std::env::var("STARTLINE_TEST_UPDATE_URL").ok()
}

fn download(url: &str, to: &Path, tested: bool) -> Result<(), String> {
    let mut c = hidden(system_tool("curl"));
    c.args(["-fsSL", "--retry", "2", "--connect-timeout", "20", "-o"]).arg(to).arg(url);
    if !tested {
        c.args(["--proto", "=https", "--proto-redir", "=https"]);
    }
    match c.status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(crate::trf!("The download failed ({status})", status = s)),
        Err(e) => Err(crate::trf!("Couldn't run curl: {error}", error = e)),
    }
}

fn updates_dir(service: bool) -> Option<PathBuf> {
    // The update task keeps its download in the install folder: only admins can swap a file there before it's installed.
    let dir = if service { std::env::current_exe().ok()?.parent()?.join("updates") } else { data_dir().join("updates") };
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn fetch_manifest(dir: &Path, service: bool) -> Result<Manifest, String> {
    let test = base_url(service);
    let base = test.clone().map(|u| u.trim_end_matches('/').to_string()).unwrap_or_else(|| format!("{RELEASES}/latest/download"));
    let (list, sig) = (dir.join("latest.json"), dir.join("latest.json.sig"));
    download(&format!("{base}/latest.json"), &list, test.is_some())?;
    download(&format!("{base}/latest.json.sig"), &sig, test.is_some())?;
    let read = |p: &Path| std::fs::read(p).map_err(|e| e.to_string());
    verified_manifest(&read(&list)?, &read(&sig)?)
}

/// Downloads the file this install needs and checks it against the signed list.
fn fetch_asset(manifest: &Manifest, install: &Install, dir: &Path, service: bool) -> Result<PathBuf, String> {
    let suffix = asset_suffix(install).ok_or(tr("This copy can't update itself"))?;
    let name = format!("startline-v{}-{suffix}", manifest.version);
    let hash = manifest.files.iter().find(|(n, _)| *n == name).map(|(_, h)| h.as_str()).ok_or_else(|| crate::trf!("The release has no {name}", name = name))?;
    let path = dir.join(&name);
    if sha256_of(&path).as_deref() != Some(hash) {
        let test = base_url(service);
        let url = match &test {
            Some(base) => format!("{}/{name}", base.trim_end_matches('/')),
            None => format!("{RELEASES}/download/v{}/{name}", manifest.version),
        };
        download(&url, &path, test.is_some())?;
        if sha256_of(&path).as_deref() != Some(hash) {
            let _ = std::fs::remove_file(&path);
            return Err(tr("The downloaded update didn't match its signed checksum, so it was deleted").into());
        }
    }
    Ok(path)
}

/// Removes what earlier updates left behind.
fn clean_dir(dir: &Path, keep: Option<&Path>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if keep != Some(path.as_path()) && path.is_file() {
            let _ = std::fs::remove_file(path);
        }
    }
}

// ---- Installing ----

/// Who to start again once the new version is in place.
#[derive(Clone, Debug, PartialEq)]
enum Restart {
    Nobody,
    /// Whoever ran the update, as themselves.
    Me,
    /// For a PC-wide install: these Windows sessions, each as the person signed in there.
    Sessions(Vec<u32>),
}

impl Restart {
    fn to_arg(&self) -> String {
        match self {
            Restart::Nobody => "none".into(),
            Restart::Me => "me".into(),
            Restart::Sessions(ids) => format!("sessions:{}", ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")),
        }
    }

    fn from_arg(arg: &str) -> Restart {
        match arg.strip_prefix("sessions:") {
            Some(ids) => Restart::Sessions(ids.split(',').filter_map(|id| id.parse().ok()).collect()),
            None if arg == "me" => Restart::Me,
            None => Restart::Nobody,
        }
    }
}

fn install_to_arg(install: &Install) -> &'static str {
    match install {
        Install::PerUser { msi: true } => "user-msi",
        Install::PerUser { msi: false } => "user-setup",
        Install::Machine { msi: true } => "machine-msi",
        Install::Machine { msi: false } => "machine-setup",
        _ => "other",
    }
}

fn install_from_arg(arg: &str) -> Install {
    match arg {
        "user-msi" => Install::PerUser { msi: true },
        "user-setup" => Install::PerUser { msi: false },
        "machine-msi" => Install::Machine { msi: true },
        "machine-setup" => Install::Machine { msi: false },
        _ => Install::Unmanaged,
    }
}

/// The installer's switches. An MSI goes to the default folder unless told where this copy is, and neither can show
/// what went wrong except in a log, which goes next to the installer. Quoted by hand, as msiexec doesn't read a quoted
/// `NAME=value`.
fn installer_command(installer: &Path, install: &Install, folder: &Path) -> Result<Command, String> {
    let log = installer.with_file_name("installer.log");
    let mut c;
    match install {
        Install::PerUser { msi: true } | Install::Machine { msi: true } => {
            c = hidden(system_tool("msiexec"));
            c.arg("/i").arg(installer).args(["/qn", "/norestart"]);
            if matches!(install, Install::Machine { .. }) {
                c.arg("ALLUSERS=1");
            } else {
                c.args(["ALLUSERS=2", "MSIINSTALLPERUSER=1"]);
            }
            c.raw_arg(format!("APPLICATIONFOLDER=\"{}\"", folder.display()));
            c.arg("/l*v").arg(log);
        }
        Install::PerUser { msi: false } | Install::Machine { msi: false } => {
            c = hidden(installer);
            c.args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"]);
            c.arg(if matches!(install, Install::Machine { .. }) { "/ALLUSERS" } else { "/CURRENTUSER" });
            c.raw_arg(format!("/LOG=\"{}\"", log.display()));
        }
        _ => return Err(tr("This copy isn't installed by an installer").into()),
    }
    Ok(c)
}

/// Starts a copy of this program, under another name next to the installer, that runs the installer and then starts
/// Startline again. The installer closes every startline.exe, this one included, but not the copy.
fn hand_over(installer: &Path, install: &Install, restart: &Restart) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let folder = exe.parent().ok_or(tr("Startline's folder is unknown"))?;
    let helper = installer.with_file_name(HELPER);
    std::fs::copy(&exe, &helper).map_err(|e| crate::trf!("Couldn't prepare the update: {error}", error = e))?;
    hidden(&helper)
        .arg("--finish-update")
        .arg(installer)
        .arg(install_to_arg(install))
        .arg(folder)
        .arg(restart.to_arg())
        .spawn()
        .map_err(|e| crate::trf!("Couldn't start the update: {error}", error = e))?;
    log(&format!("handed {} over to the helper, restart {restart:?}", installer.display()));
    Ok(())
}

/// `--finish-update <installer> <kind> <folder> <restart>`, run by the helper copy.
fn finish_update(args: &[String]) -> i32 {
    let (Some(installer), Some(kind), Some(folder), Some(restart)) = (args.get(1), args.get(2), args.get(3), args.get(4)) else { return 2 };
    let (installer, folder) = (PathBuf::from(installer), PathBuf::from(folder));
    let install = install_from_arg(kind);
    let result = installer_command(&installer, &install, &folder).and_then(|mut c| c.status().map_err(|e| e.to_string()));
    let worked = match result {
        Ok(status) if status.success() => {
            log(&format!("the installer finished: {}", installer.display()));
            true
        }
        Ok(status) => {
            log(&format!("the installer failed ({status}), see {}", installer.with_file_name("installer.log").display()));
            false
        }
        Err(e) => {
            log(&format!("couldn't run the installer: {e}"));
            false
        }
    };
    // Whatever happened, the person gets their menu back: the new version, or the old one if the install failed.
    let exe = folder.join("startline.exe");
    match Restart::from_arg(restart) {
        Restart::Nobody => {}
        Restart::Me => match Command::new(&exe).arg("--background").spawn() {
            Ok(_) => log("started Startline again"),
            Err(e) => log(&format!("couldn't start Startline again: {e}")),
        },
        Restart::Sessions(ids) => {
            for id in ids {
                match start_in_session(id, &exe) {
                    Ok(()) => log(&format!("started Startline again in session {id}")),
                    Err(e) => log(&format!("couldn't start Startline again in session {id}: {e}")),
                }
            }
        }
    }
    i32::from(!worked)
}

/// The Windows sessions, other than the services' own, where startline.exe is running.
fn sessions_running_startline() -> Vec<u32> {
    use windows::Win32::System::RemoteDesktop::*;
    let mut found = Vec::new();
    unsafe {
        let mut list: *mut WTS_PROCESS_INFOW = std::ptr::null_mut();
        let mut count = 0u32;
        if WTSEnumerateProcessesW(Some(WTS_CURRENT_SERVER_HANDLE), 0, 1, &mut list, &mut count).is_err() || list.is_null() {
            return found;
        }
        for info in std::slice::from_raw_parts(list, count as usize) {
            let name = info.pProcessName.to_string().unwrap_or_default();
            if info.SessionId != 0 && name.eq_ignore_ascii_case("startline.exe") && !found.contains(&info.SessionId) {
                found.push(info.SessionId);
            }
        }
        WTSFreeMemory(list.cast());
    }
    found
}

/// Starts Startline as the person signed in to a session. Only SYSTEM can do this.
fn start_in_session(session: u32, exe: &Path) -> Result<(), String> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
    use windows::Win32::System::RemoteDesktop::WTSQueryUserToken;
    use windows::Win32::System::Threading::{CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, PROCESS_INFORMATION, STARTUPINFOW};
    use windows::core::{PCWSTR, PWSTR};
    let wide = crate::platform::wide;
    unsafe {
        let mut token = HANDLE::default();
        WTSQueryUserToken(session, &mut token).map_err(|e| e.to_string())?;
        let mut environment = std::ptr::null_mut();
        let have_environment = CreateEnvironmentBlock(&mut environment, Some(token), false).is_ok();
        let mut desktop = wide("winsta0\\default");
        let startup = STARTUPINFOW { cb: size_of::<STARTUPINFOW>() as u32, lpDesktop: PWSTR(desktop.as_mut_ptr()), ..Default::default() };
        let mut process = PROCESS_INFORMATION::default();
        let program = wide(&exe.to_string_lossy());
        let mut line = wide(&format!("\"{}\" --background", exe.display()));
        let folder = exe.parent().map(|d| wide(&d.to_string_lossy()));
        let result = CreateProcessAsUserW(
            Some(token),
            PCWSTR(program.as_ptr()),
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_UNICODE_ENVIRONMENT,
            have_environment.then_some(environment.cast_const()),
            folder.as_ref().map_or(PCWSTR::null(), |f| PCWSTR(f.as_ptr())),
            &startup,
            &mut process,
        );
        if have_environment {
            let _ = DestroyEnvironmentBlock(environment);
        }
        let _ = CloseHandle(token);
        result.map_err(|e| e.to_string())?;
        let _ = CloseHandle(process.hThread);
        let _ = CloseHandle(process.hProcess);
    }
    Ok(())
}

/// Puts `new` where `target` is. A running program's file can be moved aside, but not overwritten.
fn replace_file(target: &Path, new: &Path) -> Result<(), String> {
    let old = target.with_extension("old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(target, &old).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::copy(new, target) {
        let _ = std::fs::rename(&old, target);
        return Err(e.to_string());
    }
    Ok(())
}

/// Removes what a finished update left: the file a portable copy moved aside, which can only go once the old program
/// has ended.
pub fn clean_after_update() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("old"));
    }
}

/// For a new copy started by the one it replaces: waits for that one to end, so it isn't taken for a running Startline.
pub fn wait_for(pid: u32) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
    unsafe {
        if let Ok(process) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            WaitForSingleObject(process, 15_000);
            let _ = CloseHandle(process);
        }
    }
}

// ---- The status the settings page shows ----

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Idle,
    Checking,
    UpToDate,
    /// Found, and waiting for the person to choose to install.
    Available(String),
    Downloading(String),
    /// Downloaded and checked: Startline restarts with it once the menu is out of sight.
    Ready(String),
    /// A PC-wide install: the task installs it.
    InstallsByTask(String),
    /// Being installed. Startline closes and starts again.
    Installing(String),
    Failed(String),
}

enum Order {
    Check { auto: bool },
    Download,
}

#[derive(Default)]
struct Shared {
    status: Option<Status>,
    ready: Option<PathBuf>,
}

pub struct Updater {
    shared: Arc<Mutex<Shared>>,
    orders: Sender<Order>,
    pub install: Install,
}

impl Updater {
    /// `auto` is read before each timed check, so turning it off stops the next one.
    pub fn start(ctx: egui::Context, auto: Arc<AtomicBool>) -> Updater {
        let shared: Arc<Mutex<Shared>> = Arc::default();
        let install = detect();
        log(&format!("running {CURRENT}, install type {install:?}"));
        let (orders, inbox) = channel();
        let mut worker = Worker { shared: shared.clone(), install: install.clone(), ctx: Some(ctx), manifest: None };
        let _ = std::thread::Builder::new().name("updater".into()).spawn(move || {
            let mut wait = FIRST_CHECK_AFTER;
            loop {
                match inbox.recv_timeout(wait) {
                    Ok(Order::Check { auto }) => worker.check(auto),
                    Ok(Order::Download) => worker.download(),
                    Err(RecvTimeoutError::Timeout) => {
                        wait = CHECK_EVERY;
                        if auto.load(Ordering::Relaxed) {
                            worker.check(true);
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        });
        Updater { shared, orders, install }
    }

    pub fn status(&self) -> Status {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner).status.clone().unwrap_or(Status::Idle)
    }

    pub fn check_now(&self, auto: bool) {
        let _ = self.orders.send(Order::Check { auto });
    }

    pub fn download_now(&self) {
        let _ = self.orders.send(Order::Download);
    }

    pub fn fail(&self, message: String) {
        log(&message);
        self.shared.lock().unwrap_or_else(PoisonError::into_inner).status = Some(Status::Failed(message));
    }

    /// Installs a downloaded update. On success Startline has to end, so the installer or new copy can take its place.
    pub fn finish(&self) -> Result<(), String> {
        let (path, version) = {
            let mut shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(Status::Ready(version)) = shared.status.clone() else { return Err(tr("Nothing is ready to install").into()) };
            let Some(path) = shared.ready.take() else { return Err(tr("Nothing is ready to install").into()) };
            shared.status = Some(Status::Installing(version.clone()));
            (path, version)
        };
        log(&format!("installing {version}"));
        install_downloaded(&path, &self.install, true)
    }
}

/// Puts a downloaded update in place, starting the new version afterwards if `restart` is set.
fn install_downloaded(path: &Path, install: &Install, restart: bool) -> Result<(), String> {
    match install {
        Install::PerUser { .. } => hand_over(path, install, if restart { &Restart::Me } else { &Restart::Nobody }),
        Install::Portable(target) => {
            replace_file(target, path).map_err(|e| crate::trf!("Couldn't install the update: {error}", error = e))?;
            if restart {
                Command::new(target).arg("--wait-for").arg(std::process::id().to_string()).arg("--background").spawn().map_err(|e| e.to_string())?;
            }
            Ok(())
        }
        Install::Machine { .. } | Install::Unmanaged => Err(tr("This copy can't update itself").to_string()),
    }
}

struct Worker {
    shared: Arc<Mutex<Shared>>,
    install: Install,
    ctx: Option<egui::Context>,
    manifest: Option<Manifest>,
}

impl Worker {
    fn set(&self, status: Status) {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner).status = Some(status);
        if let Some(ctx) = &self.ctx {
            ctx.request_repaint();
        }
    }

    fn fail(&self, message: String) {
        log(&message);
        self.set(Status::Failed(message));
    }

    fn check(&mut self, auto: bool) {
        let policy = Policies::read();
        if !allowed(&policy) {
            return self.set(Status::Idle);
        }
        self.set(Status::Checking);
        let Some(dir) = updates_dir(false) else { return self.fail(tr("There's no folder to download updates to").into()) };
        match fetch_manifest(&dir, false) {
            Err(e) => self.fail(e),
            Ok(manifest) if !is_newer(&manifest.version, CURRENT) => {
                log(&format!("{CURRENT} is current, the newest release is {}", manifest.version));
                self.set(Status::UpToDate);
                clean_dir(&dir, None);
            }
            Ok(manifest) => {
                log(&format!("{} is available", manifest.version));
                let version = manifest.version.clone();
                self.manifest = Some(manifest);
                match &self.install {
                    // the scheduled task does this without anyone here being asked
                    Install::Machine { .. } if machine_may_update(&policy, machine_switch()) => self.set(Status::InstallsByTask(version)),
                    Install::Machine { .. } | Install::Unmanaged => self.set(Status::Available(version)),
                    _ if auto => self.download(),
                    _ => self.set(Status::Available(version)),
                }
            }
        }
    }

    fn download(&mut self) {
        if !allowed(&Policies::read()) {
            return self.set(Status::Idle);
        }
        let Some(manifest) = self.manifest.take() else { return };
        let version = manifest.version.clone();
        let Some(dir) = updates_dir(false) else { return self.fail(tr("There's no folder to download updates to").into()) };
        self.set(Status::Downloading(version.clone()));
        match fetch_asset(&manifest, &self.install, &dir, false) {
            Ok(path) => {
                clean_dir(&dir, Some(&path));
                log(&format!("{version} is downloaded and checked"));
                self.shared.lock().unwrap_or_else(PoisonError::into_inner).ready = Some(path);
                self.set(Status::Ready(version));
            }
            Err(e) => {
                self.manifest = Some(manifest);
                self.fail(e);
            }
        }
    }
}

// ---- The scheduled task, and other command line runs ----

/// What the scheduled task runs. Nobody is watching, so it only logs.
fn run_service() -> i32 {
    let install = detect();
    log(&format!("update service: running {CURRENT}, install type {install:?}"));
    if !matches!(install, Install::Machine { .. }) {
        log("update service: this isn't a PC-wide install");
        return 1;
    }
    if !machine_may_update(&Policies::read(), machine_switch()) {
        log("update service: automatic updates are off");
        return 0;
    }
    let Some(dir) = updates_dir(true) else { return 1 };
    let manifest = match fetch_manifest(&dir, true) {
        Ok(m) => m,
        Err(e) => {
            log(&format!("update service: {e}"));
            return 1;
        }
    };
    if !is_newer(&manifest.version, CURRENT) {
        clean_dir(&dir, None);
        return 0;
    }
    let path = match fetch_asset(&manifest, &install, &dir, true) {
        Ok(p) => p,
        Err(e) => {
            log(&format!("update service: {e}"));
            return 1;
        }
    };
    clean_dir(&dir, Some(&path));
    let sessions = sessions_running_startline();
    log(&format!("update service: installing {}, then starting Startline again in sessions {sessions:?}", manifest.version));
    match hand_over(&path, &install, &Restart::Sessions(sessions)) {
        Ok(()) => 0,
        Err(e) => {
            log(&format!("update service: {e}"));
            1
        }
    }
}

/// `--update-now`: everything the program does about an update, without a window, for scripts. A PC-wide install
/// leaves the installing to the task, and asks for no rights it doesn't have. Nothing is started afterwards.
fn run_now() -> i32 {
    if !allowed(&Policies::read()) {
        println!("Updates are managed by the administrator");
        return 0;
    }
    let install = detect();
    let shared: Arc<Mutex<Shared>> = Arc::default();
    let mut worker = Worker { shared: shared.clone(), install: install.clone(), ctx: None, manifest: None };
    worker.check(true);
    let (status, ready) = {
        let mut shared = shared.lock().unwrap_or_else(PoisonError::into_inner);
        (shared.status.clone().unwrap_or(Status::Idle), shared.ready.take())
    };
    let status = match (status, ready) {
        (Status::Ready(version), Some(path)) => match install_downloaded(&path, &install, false) {
            Ok(()) => Status::Installing(version),
            Err(e) => Status::Failed(e),
        },
        (status, _) => status,
    };
    println!("{status:?}");
    i32::from(matches!(status, Status::Failed(_)))
}

/// Runs that do their job without a window. Returns the exit code if this was one.
pub fn handle_command_line() -> Option<i32> {
    let args: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    match args.first().map(String::as_str) {
        Some("--version") => {
            println!("Startline {CURRENT}");
            Some(0)
        }
        Some("--update-service") => Some(run_service()),
        Some("--update-now") => Some(run_now()),
        Some("--finish-update") => Some(finish_update(&args)),
        // for the update test: signs a list with the test build's published key
        #[cfg(feature = "update-test")]
        Some("--test-sign") => {
            use ed25519_dalek::Signer;
            let text = std::fs::read(args.get(1)?).ok()?;
            let signature = ed25519_dalek::SigningKey::from_bytes(&TEST_SEED).sign(&text);
            std::fs::write(args.get(2)?, signature.to_bytes()).ok()?;
            Some(0)
        }
        // for the release workflow: is this list signed by the key built into this program?
        Some("--verify-manifest") => {
            let read = |i: usize| args.get(i).and_then(|p| std::fs::read(p).ok()).unwrap_or_default();
            match verified_manifest(&read(1), &read(2)) {
                Ok(m) => {
                    println!("signature good: version {}, {} files", m.version, m.files.len());
                    Some(0)
                }
                Err(e) => {
                    eprintln!("{e}");
                    Some(1)
                }
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 8032, section 7.1, test 2.
    const KEY: [u8; 32] = [
        0x3d, 0x40, 0x17, 0xc3, 0xe8, 0x43, 0x89, 0x5a, 0x92, 0xb7, 0x0a, 0xa7, 0x4d, 0x1b, 0x7e, 0xbc, 0x9c, 0x98, 0x2c, 0xcf, 0x2e, 0xc4, 0x96, 0x8c, 0xc0, 0xcd, 0x55, 0xf1, 0x2a, 0xf4, 0x66, 0x0c,
    ];
    const SIG: [u8; 64] = [
        0x92, 0xa0, 0x09, 0xa9, 0xf0, 0xd4, 0xca, 0xb8, 0x72, 0x0e, 0x82, 0x0b, 0x5f, 0x64, 0x25, 0x40, 0xa2, 0xb2, 0x7b, 0x54, 0x16, 0x50, 0x3f, 0x8f, 0xb3, 0x76, 0x22, 0x23, 0xeb, 0xdb, 0x69, 0xda, 0x08, 0x5a, 0xc1, 0xe4, 0x3e, 0x15, 0x99, 0x6e, 0x45, 0x8f, 0x36, 0x13, 0xd0, 0xf1, 0x1d, 0x8c, 0x38, 0x7b, 0x2e, 0xae, 0xb4, 0x30, 0x2a, 0xee, 0xb0, 0x0d, 0x29, 0x16, 0x12, 0xbb, 0x0c, 0x00,
    ];

    #[test]
    fn a_good_signature_passes_and_anything_changed_fails() {
        assert!(signature_ok(&KEY, &[0x72], &SIG));
        assert!(!signature_ok(&KEY, &[0x73], &SIG), "a changed message");
        let mut bad = SIG;
        bad[0] ^= 1;
        assert!(!signature_ok(&KEY, &[0x72], &bad), "a changed signature");
        assert!(!signature_ok(&KEY, &[0x72], &SIG[..63]), "a short signature");
        assert!(!signature_ok(&PUBLIC_KEY, &[0x72], &SIG), "another key");
    }

    #[test]
    fn the_real_keys_are_valid_keys() {
        assert!(VerifyingKey::from_bytes(&PUBLIC_KEY).is_ok());
        assert!(VerifyingKey::from_bytes(&BACKUP_PUBLIC_KEY).is_ok());
        assert_ne!(PUBLIC_KEY, BACKUP_PUBLIC_KEY);
    }

    #[cfg(not(feature = "update-test"))]
    #[test]
    fn the_real_program_refuses_the_test_builds_key() {
        use ed25519_dalek::Signer;
        let text = b"version 99.0.0\n";
        let signature = ed25519_dalek::SigningKey::from_bytes(&[7; 32]).sign(text);
        assert!(verified_manifest(text, &signature.to_bytes()).is_err());
    }

    #[test]
    fn unsigned_lists_are_refused() {
        assert!(verified_manifest(b"version 99.0.0\n", &[0; 64]).is_err());
    }

    #[test]
    fn administrators_decide_before_the_switch() {
        let policies = |values: serde_json::Value| Policies::from_raw(&policy::tests_raw(values));
        let may = machine_may_update;
        assert!(may(&Policies::default(), None), "on unless someone says otherwise");
        assert!(!may(&Policies::default(), Some(false)));
        assert!(may(&policies(serde_json::json!({ "AutomaticUpdates": 1 })), Some(false)), "a policy's on beats the switch");
        assert!(!may(&policies(serde_json::json!({ "AutomaticUpdates": 0 })), Some(true)), "a policy's off beats the switch");
        assert!(!may(&policies(serde_json::json!({ "CheckForUpdates": 0, "AutomaticUpdates": 1 })), Some(true)), "turned off altogether");
    }

    #[test]
    fn versions_compare_as_numbers() {
        assert!(is_newer("0.0.10", "0.0.9"));
        assert!(is_newer("0.1.0", "0.0.99"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(is_newer("v0.0.2", "0.0.1"));
        assert!(!is_newer("0.0.1", "0.0.1"), "the same version");
        assert!(!is_newer("0.0.1", "0.0.2"), "older");
        assert!(!is_newer("nonsense", "0.0.1"));
        assert!(!is_newer("1.2", "0.0.1"));
    }

    #[test]
    fn lists_are_read_and_rubbish_is_refused() {
        let m = parse_manifest("version 1.2.3\nfile ABC123 startline-v1.2.3-windows-x64.msi\nfile def456 name with spaces.zip\n").expect("reads");
        assert_eq!(m.version, "1.2.3");
        assert_eq!(m.files[0], ("startline-v1.2.3-windows-x64.msi".to_string(), "abc123".to_string()));
        assert_eq!(m.files[1].0, "name with spaces.zip");
        assert!(parse_manifest("file abc name\n").is_none(), "no version");
        assert!(parse_manifest("version soon\n").is_none(), "a version that isn't one");
    }

    #[test]
    fn what_the_helper_is_told_reads_back_the_same() {
        for restart in [Restart::Nobody, Restart::Me, Restart::Sessions(vec![1, 4])] {
            assert_eq!(Restart::from_arg(&restart.to_arg()), restart);
        }
        for install in [Install::PerUser { msi: true }, Install::PerUser { msi: false }, Install::Machine { msi: true }, Install::Machine { msi: false }] {
            assert_eq!(install_from_arg(install_to_arg(&install)), install);
        }
    }
}
