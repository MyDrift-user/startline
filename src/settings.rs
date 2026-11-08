//! Reads and changes the real Windows settings from search. Changes run on a helper
//! thread: a monitor can take a moment to answer, and the menu must never wait for it.

use crate::platform;
pub use crate::setting_list::{Control, QuickId, Value};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use windows::Devices::Radios::*;
use windows::Win32::Devices::Display::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Media::Audio::Endpoints::*;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::System::Power::*;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Rpc::*;
use windows::Win32::System::SystemServices::{GUID_SLEEP_SUBGROUP, GUID_STANDBY_TIMEOUT, GUID_VIDEO_POWERDOWN_TIMEOUT, GUID_VIDEO_SUBGROUP};
use windows::Win32::System::Variant::*;
use windows::Win32::System::Wmi::*;
use windows::Win32::UI::Accessibility::{SKF_STICKYKEYSON, STICKYKEYS};
use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

/// Who has to hear about a registry change for it to show.
#[derive(Clone, Copy)]
enum Notify {
    Nothing,
    Theme,
    Shell,
    Taskbar,
}

/// A per-user on/off value in the registry.
struct RegFlag {
    key: &'static str,
    name: &'static str,
    on: u32,
    off: u32,
    when_missing: bool,
    notify: Notify,
}

fn reg_flag(id: QuickId) -> Option<RegFlag> {
    let flag = |key, name, on, off, when_missing, notify| Some(RegFlag { key, name, on, off, when_missing, notify });
    match id {
        QuickId::AppsDark => flag(PERSONALIZE, "AppsUseLightTheme", 0, 1, false, Notify::Theme),
        QuickId::SystemDark => flag(PERSONALIZE, "SystemUsesLightTheme", 0, 1, false, Notify::Theme),
        QuickId::Transparency => flag(PERSONALIZE, "EnableTransparency", 1, 0, true, Notify::Theme),
        QuickId::AccentTaskbar => flag(PERSONALIZE, "ColorPrevalence", 1, 0, false, Notify::Theme),
        QuickId::AccentTitleBars => flag("Software\\Microsoft\\Windows\\DWM", "ColorPrevalence", 1, 0, false, Notify::Theme),
        QuickId::TaskViewButton => flag(EXPLORER_ADVANCED, "ShowTaskViewButton", 1, 0, true, Notify::Taskbar),
        QuickId::ClockSeconds => flag(EXPLORER_ADVANCED, "ShowSecondsInSystemClock", 1, 0, false, Notify::Taskbar),
        QuickId::FileExtensions => flag(EXPLORER_ADVANCED, "HideFileExt", 0, 1, false, Notify::Shell),
        QuickId::HiddenFiles => flag(EXPLORER_ADVANCED, "Hidden", 1, 2, false, Notify::Shell),
        QuickId::ItemCheckboxes => flag(EXPLORER_ADVANCED, "AutoCheckSelect", 1, 0, false, Notify::Shell),
        QuickId::CompactExplorer => flag(EXPLORER_ADVANCED, "UseCompactMode", 1, 0, false, Notify::Shell),
        QuickId::ClipboardHistory => flag("Software\\Microsoft\\Clipboard", "EnableClipboardHistory", 1, 0, false, Notify::Nothing),
        QuickId::GameMode => flag("Software\\Microsoft\\GameBar", "AutoGameModeEnabled", 1, 0, true, Notify::Nothing),
        _ => None,
    }
}

impl RegFlag {
    fn read(&self) -> Value {
        Value::On(reg_dword(self.key, self.name).map_or(self.when_missing, |v| v == self.on))
    }

    fn write(&self, on: bool) -> bool {
        let ok = reg_set_dword(self.key, self.name, if on { self.on } else { self.off });
        match self.notify {
            Notify::Nothing => {}
            Notify::Theme => broadcast_change("ImmersiveColorSet"),
            Notify::Shell => refresh_explorer(),
            Notify::Taskbar => broadcast_change("TraySettings"),
        }
        ok
    }
}

/// An on/off value Windows hands out through its system parameters.
struct SpiFlag {
    get: SYSTEM_PARAMETERS_INFO_ACTION,
    set: SYSTEM_PARAMETERS_INFO_ACTION,
    /// Some settings take the new value in the first parameter, the others in the second.
    in_first_param: bool,
}

fn spi_flag(id: QuickId) -> Option<SpiFlag> {
    let flag = |get, set, in_first_param| Some(SpiFlag { get, set, in_first_param });
    match id {
        QuickId::Animations => flag(SPI_GETCLIENTAREAANIMATION, SPI_SETCLIENTAREAANIMATION, false),
        QuickId::SnapWindows => flag(SPI_GETWINARRANGING, SPI_SETWINARRANGING, false),
        QuickId::DragContents => flag(SPI_GETDRAGFULLWINDOWS, SPI_SETDRAGFULLWINDOWS, true),
        QuickId::FontSmoothing => flag(SPI_GETFONTSMOOTHING, SPI_SETFONTSMOOTHING, true),
        QuickId::WindowShadows => flag(SPI_GETDROPSHADOW, SPI_SETDROPSHADOW, false),
        QuickId::HoverActivate => flag(SPI_GETACTIVEWINDOWTRACKING, SPI_SETACTIVEWINDOWTRACKING, false),
        QuickId::ScreenSaver => flag(SPI_GETSCREENSAVEACTIVE, SPI_SETSCREENSAVEACTIVE, true),
        _ => None,
    }
}

const SAVE_AND_TELL: SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS = SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(SPIF_UPDATEINIFILE.0 | SPIF_SENDCHANGE.0);

impl SpiFlag {
    fn read(&self) -> Value {
        let mut on = BOOL(0);
        if spi_read(self.get, &mut on) { Value::On(on.as_bool()) } else { Value::Unavailable }
    }

    fn write(&self, on: bool) -> bool {
        if self.in_first_param {
            spi_write_first(self.set, on as u32)
        } else {
            unsafe { SystemParametersInfoW(self.set, 0, Some(on as usize as *mut c_void), SAVE_AND_TELL) }.is_ok()
        }
    }
}

fn spi_read<T>(action: SYSTEM_PARAMETERS_INFO_ACTION, out: &mut T) -> bool {
    unsafe { SystemParametersInfoW(action, 0, Some(out as *mut T as *mut c_void), Default::default()) }.is_ok()
}

fn spi_write_first(action: SYSTEM_PARAMETERS_INFO_ACTION, value: u32) -> bool {
    unsafe { SystemParametersInfoW(action, value, None, SAVE_AND_TELL) }.is_ok()
}

fn spi_number(action: SYSTEM_PARAMETERS_INFO_ACTION) -> Value {
    let mut number = 0u32;
    if spi_read(action, &mut number) { Value::Number(number as f32) } else { Value::Unavailable }
}

fn sticky_keys() -> Option<STICKYKEYS> {
    let size = std::mem::size_of::<STICKYKEYS>() as u32;
    let mut keys = STICKYKEYS { cbSize: size, ..Default::default() };
    unsafe { SystemParametersInfoW(SPI_GETSTICKYKEYS, size, Some(&mut keys as *mut _ as *mut c_void), Default::default()) }.ok()?;
    Some(keys)
}

fn sticky_keys_set(on: bool) -> bool {
    let Some(mut keys) = sticky_keys() else { return false };
    keys.dwFlags.0 = if on { keys.dwFlags.0 | SKF_STICKYKEYSON.0 } else { keys.dwFlags.0 & !SKF_STICKYKEYSON.0 };
    let size = std::mem::size_of::<STICKYKEYS>() as u32;
    unsafe { SystemParametersInfoW(SPI_SETSTICKYKEYS, size, Some(&mut keys as *mut _ as *mut c_void), SAVE_AND_TELL) }.is_ok()
}

/// Windows' "enhance pointer precision" is the third of the three mouse numbers.
fn pointer_precision_get() -> Value {
    let mut mouse = [0i32; 3];
    if spi_read(SPI_GETMOUSE, &mut mouse) { Value::On(mouse[2] != 0) } else { Value::Unavailable }
}

fn pointer_precision_set(on: bool) -> bool {
    let mut mouse: [i32; 3] = if on { [6, 10, 1] } else { [0, 0, 0] };
    unsafe { SystemParametersInfoW(SPI_SETMOUSE, 0, Some(mouse.as_mut_ptr() as *mut c_void), SAVE_AND_TELL) }.is_ok()
}

// ---- Screen and sleep timeouts, in the active power plan ----

fn active_power_scheme() -> Option<GUID> {
    let mut found: *mut GUID = std::ptr::null_mut();
    if unsafe { PowerGetActiveScheme(None, &mut found) } != ERROR_SUCCESS || found.is_null() {
        return None;
    }
    let scheme = unsafe { *found };
    let _ = unsafe { LocalFree(Some(HLOCAL(found as *mut c_void))) };
    Some(scheme)
}

fn power_minutes_get(group: &GUID, setting: &GUID) -> Value {
    let Some(scheme) = active_power_scheme() else { return Value::Unavailable };
    let mut seconds = 0u32;
    let status = unsafe { PowerReadACValueIndex(None, Some(&scheme), Some(group), Some(setting), &mut seconds) };
    if status == ERROR_SUCCESS { Value::Number((seconds as f32 / 60.0).round()) } else { Value::Unavailable }
}

/// Changes it both plugged in and on battery, then makes Windows pick up the change.
fn power_minutes_set(group: &GUID, setting: &GUID, minutes: f32) -> bool {
    let Some(scheme) = active_power_scheme() else { return false };
    let seconds = minutes.round().max(0.0) as u32 * 60;
    unsafe {
        let plugged_in = PowerWriteACValueIndex(None, &scheme, Some(group), Some(setting), seconds);
        let battery = PowerWriteDCValueIndex(None, &scheme, Some(group), Some(setting), seconds);
        plugged_in == ERROR_SUCCESS && battery == 0 && PowerSetActiveScheme(None, Some(&scheme)) == ERROR_SUCCESS
    }
}

enum Job {
    RefreshAll,
    Set(QuickId, Value),
}

#[derive(Clone)]
pub struct Settings {
    values: Arc<Mutex<HashMap<QuickId, Value>>>,
    tx: Sender<Job>,
}

impl Settings {
    pub fn start(ctx: egui::Context) -> Settings {
        let values: Arc<Mutex<HashMap<QuickId, Value>>> = Arc::default();
        let (tx, rx) = mpsc::channel::<Job>();
        let shared = values.clone();
        std::thread::Builder::new()
            .name("settings".into())
            .spawn(move || {
                let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                let mut backend = Backend::default();
                while let Ok(first) = rx.recv() {
                    // Only the last position of a dragged slider matters.
                    let mut jobs = vec![first];
                    jobs.extend(rx.try_iter());
                    let mut latest: HashMap<QuickId, Value> = HashMap::new();
                    let mut refresh = false;
                    for job in jobs {
                        match job {
                            Job::RefreshAll => refresh = true,
                            Job::Set(id, value) => {
                                latest.insert(id, value);
                            }
                        }
                    }
                    for (id, value) in latest {
                        backend.set(id, value);
                        let confirmed = backend.get(id);
                        shared.lock().unwrap().insert(id, confirmed);
                        ctx.request_repaint();
                    }
                    if refresh {
                        for id in QuickId::ALL {
                            let value = backend.get(id);
                            shared.lock().unwrap().insert(id, value);
                            ctx.request_repaint();
                        }
                    }
                }
            })
            .expect("settings thread");
        let settings = Settings { values, tx };
        settings.refresh();
        settings
    }

    pub fn refresh(&self) {
        let _ = self.tx.send(Job::RefreshAll);
    }

    pub fn get(&self, id: QuickId) -> Value {
        self.values.lock().unwrap().get(&id).copied().unwrap_or(Value::Loading)
    }

    pub fn set(&self, id: QuickId, value: Value) {
        // Show the new value right away; the helper corrects it if Windows disagrees.
        self.values.lock().unwrap().insert(id, value);
        let _ = self.tx.send(Job::Set(id, value));
    }

    pub fn is_settled(&self) -> bool {
        let values = self.values.lock().unwrap();
        QuickId::ALL.iter().all(|id| !matches!(values.get(id), None | Some(Value::Loading)))
    }
}

/// Reads every setting and reports it. It only looks: nothing on the PC is changed.
pub fn probe_report() -> String {
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let mut backend = Backend::default();
    let mut out = String::new();
    for id in QuickId::ALL {
        let started = std::time::Instant::now();
        let value = backend.get(id);
        let read_ms = started.elapsed().as_millis();
        out.push_str(&format!("{:<20} {value:?} read in {read_ms}ms\n", id.info().label));
    }
    out
}

/// Changes each setting that exists on this PC, reads it back and puts it straight back,
/// proving the change really reaches Windows.
pub fn probe_changes_report() -> String {
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let mut backend = Backend::default();
    let mut out = String::new();
    // Dark mode writes two values but reads one, so keep the other to restore it exactly.
    let system_theme = reg_dword(PERSONALIZE, "SystemUsesLightTheme");
    for id in QuickId::ALL {
        let before = backend.get(id);
        let changed = match (id.info().control, before) {
            (_, Value::On(on)) => Value::On(!on),
            (Control::Slider { min, .. }, Value::Number(v)) => Value::Number(if v > min { v - 1.0 } else { v + 1.0 }),
            (Control::Choice(options), Value::Pick(i)) => Value::Pick((i + 1) % options.len()),
            _ => {
                out.push_str(&format!("{:<20} skipped: {before:?}\n", id.info().label));
                continue;
            }
        };
        backend.set(id, changed);
        let after = backend.get(id);
        backend.set(id, before);
        if id == QuickId::DarkMode {
            if let Some(value) = system_theme {
                reg_set_dword(PERSONALIZE, "SystemUsesLightTheme", value);
                broadcast_change("ImmersiveColorSet");
            }
        }
        let restored = backend.get(id);
        let verdict = if after == changed && restored == before { "works" } else { "FAILED" };
        out.push_str(&format!("{:<20} {verdict}: {before:?} -> {after:?} -> {restored:?}\n", id.info().label));
    }
    out
}

#[derive(Default)]
struct Backend {
    speaker: Option<IAudioEndpointVolume>,
    mic: Option<IAudioEndpointVolume>,
    wmi: Option<IWbemServices>,
    keep_awake: bool,
}

impl Backend {
    fn get(&mut self, id: QuickId) -> Value {
        if let Some(flag) = reg_flag(id) {
            return flag.read();
        }
        if let Some(flag) = spi_flag(id) {
            return flag.read();
        }
        match id {
            QuickId::TaskbarAlign => Value::Pick(reg_dword(EXPLORER_ADVANCED, "TaskbarAl").map_or(1, |v| v.min(1) as usize)),
            QuickId::PointerPrecision => pointer_precision_get(),
            QuickId::SwapMouseButtons => Value::On(unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0),
            QuickId::StickyKeys => sticky_keys().map_or(Value::Unavailable, |keys| Value::On(keys.dwFlags.0 & SKF_STICKYKEYSON.0 != 0)),
            QuickId::Autostart => Value::On(autostart_enabled()),
            QuickId::ScrollLines => spi_number(SPI_GETWHEELSCROLLLINES),
            QuickId::DoubleClick => Value::Number(unsafe { GetDoubleClickTime() } as f32),
            QuickId::KeyRepeatSpeed => spi_number(SPI_GETKEYBOARDSPEED),
            QuickId::KeyRepeatDelay => {
                let mut delay = 0i32;
                if spi_read(SPI_GETKEYBOARDDELAY, &mut delay) { Value::Pick(delay.clamp(0, 3) as usize) } else { Value::Unavailable }
            }
            QuickId::ScreenTimeout => power_minutes_get(&GUID_VIDEO_SUBGROUP, &GUID_VIDEO_POWERDOWN_TIMEOUT),
            QuickId::SleepTimeout => power_minutes_get(&GUID_SLEEP_SUBGROUP, &GUID_STANDBY_TIMEOUT),
            QuickId::Wifi => radio_get(RadioKind::WiFi),
            QuickId::Bluetooth => radio_get(RadioKind::Bluetooth),
            QuickId::DarkMode => reg_dword(PERSONALIZE, "AppsUseLightTheme").map_or(Value::Unavailable, |v| Value::On(v == 0)),
            QuickId::KeepAwake => Value::On(self.keep_awake),
            QuickId::Mute => self.endpoint(false).and_then(|e| unsafe { e.GetMute() }.ok()).map_or(Value::Unavailable, |m| Value::On(m.as_bool())),
            QuickId::MicMute => self.endpoint(true).and_then(|e| unsafe { e.GetMute() }.ok()).map_or(Value::Unavailable, |m| Value::On(m.as_bool())),
            QuickId::Volume => volume_get(self.endpoint(false)),
            QuickId::MicVolume => volume_get(self.endpoint(true)),
            QuickId::AutoHideTaskbar => {
                let mut data = APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, ..Default::default() };
                let state = unsafe { SHAppBarMessage(ABM_GETSTATE, &mut data) };
                Value::On(state & ABS_AUTOHIDE as usize != 0)
            }
            QuickId::Brightness => self.brightness_get(),
            QuickId::MouseSpeed => {
                let mut speed = 0i32;
                let ok = unsafe { SystemParametersInfoW(SPI_GETMOUSESPEED, 0, Some(&mut speed as *mut _ as *mut c_void), Default::default()) };
                if ok.is_ok() { Value::Number(speed as f32) } else { Value::Unavailable }
            }
            QuickId::PowerMode => power_mode_get(),
            _ => Value::Unavailable,
        }
    }

    fn set(&mut self, id: QuickId, value: Value) -> bool {
        if let (Some(flag), Value::On(on)) = (reg_flag(id), value) {
            return flag.write(on);
        }
        if let (Some(flag), Value::On(on)) = (spi_flag(id), value) {
            return flag.write(on);
        }
        match (id, value) {
            (QuickId::TaskbarAlign, Value::Pick(i)) => {
                let ok = reg_set_dword(EXPLORER_ADVANCED, "TaskbarAl", i.min(1) as u32);
                broadcast_change("TraySettings");
                ok
            }
            (QuickId::PointerPrecision, Value::On(on)) => pointer_precision_set(on),
            (QuickId::SwapMouseButtons, Value::On(on)) => spi_write_first(SPI_SETMOUSEBUTTONSWAP, on as u32),
            (QuickId::StickyKeys, Value::On(on)) => sticky_keys_set(on),
            (QuickId::Autostart, Value::On(on)) => set_autostart(on),
            (QuickId::ScrollLines, Value::Number(v)) => spi_write_first(SPI_SETWHEELSCROLLLINES, v.round() as u32),
            (QuickId::DoubleClick, Value::Number(v)) => spi_write_first(SPI_SETDOUBLECLICKTIME, v.round() as u32),
            (QuickId::KeyRepeatSpeed, Value::Number(v)) => spi_write_first(SPI_SETKEYBOARDSPEED, v.round() as u32),
            (QuickId::KeyRepeatDelay, Value::Pick(i)) => spi_write_first(SPI_SETKEYBOARDDELAY, i.min(3) as u32),
            (QuickId::ScreenTimeout, Value::Number(v)) => power_minutes_set(&GUID_VIDEO_SUBGROUP, &GUID_VIDEO_POWERDOWN_TIMEOUT, v),
            (QuickId::SleepTimeout, Value::Number(v)) => power_minutes_set(&GUID_SLEEP_SUBGROUP, &GUID_STANDBY_TIMEOUT, v),
            (QuickId::Wifi, Value::On(on)) => radio_set(RadioKind::WiFi, on),
            (QuickId::Bluetooth, Value::On(on)) => radio_set(RadioKind::Bluetooth, on),
            (QuickId::DarkMode, Value::On(on)) => {
                let a = reg_set_dword(PERSONALIZE, "AppsUseLightTheme", !on as u32);
                let b = reg_set_dword(PERSONALIZE, "SystemUsesLightTheme", !on as u32);
                broadcast_change("ImmersiveColorSet");
                a && b
            }
            (QuickId::KeepAwake, Value::On(on)) => {
                self.keep_awake = on;
                let flags = if on { ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED } else { ES_CONTINUOUS };
                unsafe { windows::Win32::System::Power::SetThreadExecutionState(flags) }.0 != 0
            }
            (QuickId::Mute, Value::On(on)) => self.endpoint(false).is_some_and(|e| unsafe { e.SetMute(on, std::ptr::null()) }.is_ok()),
            (QuickId::MicMute, Value::On(on)) => self.endpoint(true).is_some_and(|e| unsafe { e.SetMute(on, std::ptr::null()) }.is_ok()),
            (QuickId::Volume, Value::Number(v)) => volume_set(self.endpoint(false), v),
            (QuickId::MicVolume, Value::Number(v)) => volume_set(self.endpoint(true), v),
            (QuickId::AutoHideTaskbar, Value::On(on)) => {
                let mut data = APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, ..Default::default() };
                data.lParam = LPARAM(if on { ABS_AUTOHIDE as isize } else { ABS_ALWAYSONTOP as isize });
                unsafe { SHAppBarMessage(ABM_SETSTATE, &mut data) };
                true
            }
            (QuickId::Brightness, Value::Number(v)) => self.brightness_set(v),
            (QuickId::MouseSpeed, Value::Number(v)) => unsafe {
                SystemParametersInfoW(
                    SPI_SETMOUSESPEED,
                    0,
                    Some(v.round().clamp(1.0, 20.0) as usize as *mut c_void),
                    SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
                )
            }
            .is_ok(),
            (QuickId::PowerMode, Value::Pick(i)) => power_mode_set(i),
            _ => false,
        }
    }

    fn endpoint(&mut self, mic: bool) -> Option<IAudioEndpointVolume> {
        // The default device can change (headphones plugged in), so ask afresh on a miss.
        let flow = if mic { eCapture } else { eRender };
        let fresh = unsafe {
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let device = enumerator.GetDefaultAudioEndpoint(flow, eConsole).ok()?;
            device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()?
        };
        let slot = if mic { &mut self.mic } else { &mut self.speaker };
        *slot = Some(fresh.clone());
        Some(fresh)
    }

    fn brightness_get(&mut self) -> Value {
        if let Some(value) = ddc_brightness(None) {
            return Value::Number(value);
        }
        self.wmi_brightness_get().map_or(Value::Unavailable, Value::Number)
    }

    fn brightness_set(&mut self, value: f32) -> bool {
        if ddc_brightness(Some(value)).is_some() {
            return true;
        }
        self.wmi_brightness_set(value).is_some()
    }

    fn wmi_service(&mut self) -> Option<IWbemServices> {
        if self.wmi.is_none() {
            unsafe {
                let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).ok()?;
                let service = locator
                    .ConnectServer(&BSTR::from("ROOT\\WMI"), &BSTR::new(), &BSTR::new(), &BSTR::new(), 0, &BSTR::new(), None)
                    .ok()?;
                let _ = CoSetProxyBlanket(
                    &service,
                    RPC_C_AUTHN_WINNT,
                    RPC_C_AUTHZ_NONE,
                    PCWSTR::null(),
                    RPC_C_AUTHN_LEVEL_CALL,
                    RPC_C_IMP_LEVEL_IMPERSONATE,
                    None,
                    EOAC_NONE,
                );
                self.wmi = Some(service);
            }
        }
        self.wmi.clone()
    }

    fn wmi_first(&mut self, class: &str) -> Option<IWbemClassObject> {
        let service = self.wmi_service()?;
        unsafe {
            let rows = service
                .ExecQuery(
                    &BSTR::from("WQL"),
                    &BSTR::from(format!("SELECT * FROM {class}")),
                    WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                    None,
                )
                .ok()?;
            let mut slot = [None];
            let mut returned = 0;
            rows.Next(3000, &mut slot, &mut returned).ok().ok()?;
            slot[0].take()
        }
    }

    fn wmi_brightness_get(&mut self) -> Option<f32> {
        let row = self.wmi_first("WmiMonitorBrightness")?;
        unsafe {
            let mut value = VARIANT::default();
            row.Get(w!("CurrentBrightness"), 0, &mut value, None, None).ok()?;
            i32::try_from(&value).ok().map(|v| v as f32)
        }
    }

    fn wmi_brightness_set(&mut self, percent: f32) -> Option<()> {
        let row = self.wmi_first("WmiMonitorBrightnessMethods")?;
        let service = self.wmi_service()?;
        unsafe {
            let mut path = VARIANT::default();
            row.Get(w!("__PATH"), 0, &mut path, None, None).ok()?;
            let path = BSTR::try_from(&path).ok()?;
            let mut class = None;
            service
                .GetObject(&BSTR::from("WmiMonitorBrightnessMethods"), Default::default(), None, Some(&mut class), None)
                .ok()?;
            let class = class?;
            let mut in_def = None;
            class.GetMethod(w!("WmiSetBrightness"), 0, &mut in_def, std::ptr::null_mut()).ok()?;
            let params = in_def?.SpawnInstance(0).ok()?;
            params.Put(w!("Timeout"), 0, &VARIANT::from(1i32), 0).ok()?;
            params.Put(w!("Brightness"), 0, &VARIANT::from(percent.round() as i32), 0).ok()?;
            service
                .ExecMethod(&path, &BSTR::from("WmiSetBrightness"), Default::default(), None, &params, None, None)
                .ok()
        }
    }
}

fn volume_get(endpoint: Option<IAudioEndpointVolume>) -> Value {
    endpoint
        .and_then(|e| unsafe { e.GetMasterVolumeLevelScalar() }.ok())
        .map_or(Value::Unavailable, |v| Value::Number((v * 100.0).round()))
}

fn volume_set(endpoint: Option<IAudioEndpointVolume>, percent: f32) -> bool {
    endpoint.is_some_and(|e| unsafe { e.SetMasterVolumeLevelScalar((percent / 100.0).clamp(0.0, 1.0), std::ptr::null()) }.is_ok())
}

// ---- Wi-Fi and Bluetooth ----

fn find_radio(kind: RadioKind) -> Option<Radio> {
    let _ = Radio::RequestAccessAsync().ok()?.join().ok()?;
    let radios = Radio::GetRadiosAsync().ok()?.join().ok()?;
    let count = radios.Size().ok()?;
    (0..count).filter_map(|i| radios.GetAt(i).ok()).find(|r| r.Kind().ok() == Some(kind))
}

fn radio_get(kind: RadioKind) -> Value {
    find_radio(kind)
        .and_then(|r| r.State().ok())
        .map_or(Value::Unavailable, |state| Value::On(state == RadioState::On))
}

fn radio_set(kind: RadioKind, on: bool) -> bool {
    let wanted = if on { RadioState::On } else { RadioState::Off };
    find_radio(kind)
        .and_then(|r| r.SetStateAsync(wanted).ok())
        .and_then(|operation| operation.join().ok())
        .is_some_and(|status| status == RadioAccessStatus::Allowed)
}

// ---- Registry ----

const PERSONALIZE: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
const EXPLORER_ADVANCED: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced";

pub(crate) fn reg_dword(key: &str, name: &str) -> Option<u32> {
    let (key_w, name_w) = (platform::wide(key), platform::wide(name));
    let mut value = 0u32;
    let mut size = 4u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key_w.as_ptr()),
            PCWSTR(name_w.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut _ as *mut c_void),
            Some(&mut size),
        )
    };
    status.is_ok().then_some(value)
}

fn reg_set_dword(key: &str, name: &str, value: u32) -> bool {
    let (key_w, name_w) = (platform::wide(key), platform::wide(name));
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key_w.as_ptr()),
            PCWSTR(name_w.as_ptr()),
            REG_DWORD.0,
            Some(&value as *const _ as *const c_void),
            4,
        )
    }
    .is_ok()
}

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

/// Whether the menu starts together with Windows.
pub fn autostart_enabled() -> bool {
    let (key_w, name_w) = (platform::wide(RUN_KEY), platform::wide("Startline"));
    let mut size = 0u32;
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, PCWSTR(key_w.as_ptr()), PCWSTR(name_w.as_ptr()), RRF_RT_REG_SZ, None, None, Some(&mut size))
    }
    .is_ok()
}

pub fn set_autostart(on: bool) -> bool {
    let (key_w, name_w) = (platform::wide(RUN_KEY), platform::wide("Startline"));
    if !on {
        return unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(key_w.as_ptr()), PCWSTR(name_w.as_ptr())) }.is_ok();
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    let command = platform::wide(&format!("\"{}\" --background", exe.display()));
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key_w.as_ptr()),
            PCWSTR(name_w.as_ptr()),
            REG_SZ.0,
            Some(command.as_ptr() as *const c_void),
            (command.len() * 2) as u32,
        )
    }
    .is_ok()
}

fn broadcast_change(what: &str) {
    let text = platform::wide(what);
    unsafe {
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(text.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            200,
            None,
        );
    }
}

fn refresh_explorer() {
    broadcast_change("ShellState");
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}

// ---- Monitor brightness over the display cable ----

fn ddc_brightness(set: Option<f32>) -> Option<f32> {
    struct Found {
        value: Option<f32>,
        set: Option<f32>,
    }
    unsafe extern "system" fn each(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        unsafe {
            let found = &mut *(data.0 as *mut Found);
            let mut count = 0u32;
            if GetNumberOfPhysicalMonitorsFromHMONITOR(monitor, &mut count).is_err() || count == 0 {
                return TRUE;
            }
            let mut screens = vec![PHYSICAL_MONITOR::default(); count as usize];
            if GetPhysicalMonitorsFromHMONITOR(monitor, &mut screens).is_ok() {
                for screen in &screens {
                    let (mut low, mut current, mut high) = (0u32, 0u32, 0u32);
                    if GetMonitorBrightness(screen.hPhysicalMonitor, &mut low, &mut current, &mut high) != 0 && high > low {
                        if let Some(percent) = found.set {
                            let raw = low as f32 + (high - low) as f32 * percent.clamp(0.0, 100.0) / 100.0;
                            if SetMonitorBrightness(screen.hPhysicalMonitor, raw.round() as u32) != 0 {
                                found.value = Some(percent);
                            }
                        } else if found.value.is_none() {
                            found.value = Some(((current - low) as f32 * 100.0 / (high - low) as f32).round());
                        }
                    }
                }
                let _ = DestroyPhysicalMonitors(&screens);
            }
            TRUE
        }
    }
    let mut found = Found { value: None, set };
    let _ = unsafe { EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut found as *mut Found as isize)) };
    found.value
}

// ---- Power mode ----

const POWER_MODES: [GUID; 3] = [
    GUID::from_u128(0x961cc777_2547_4f9d_8174_7d86181b8a7a), // best efficiency
    GUID::from_u128(0x00000000_0000_0000_0000_000000000000), // balanced
    GUID::from_u128(0xded574b5_45a0_4f42_8737_46345c09c238), // best performance
];

fn power_fn(name: PCSTR) -> Option<unsafe extern "system" fn() -> isize> {
    unsafe {
        let lib = LoadLibraryW(w!("powrprof.dll")).ok()?;
        GetProcAddress(lib, name)
    }
}

fn power_mode_get() -> Value {
    let Some(raw) = power_fn(s!("PowerGetEffectiveOverlayScheme")) else { return Value::Unavailable };
    let get: unsafe extern "system" fn(*mut GUID) -> u32 = unsafe { std::mem::transmute(raw) };
    let mut guid = GUID::zeroed();
    if unsafe { get(&mut guid) } != 0 {
        return Value::Unavailable;
    }
    Value::Pick(POWER_MODES.iter().position(|m| *m == guid).unwrap_or(1))
}

fn power_mode_set(index: usize) -> bool {
    let Some(raw) = power_fn(s!("PowerSetActiveOverlayScheme")) else { return false };
    let set: unsafe extern "system" fn(*const GUID) -> u32 = unsafe { std::mem::transmute(raw) };
    POWER_MODES.get(index).is_some_and(|guid| unsafe { set(guid) } == 0)
}
