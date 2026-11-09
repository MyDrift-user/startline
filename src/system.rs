//! The system connection: makes the Start key and Start button open this menu, shows the
//! tray icon, and places the window. One thread watches the keyboard and mouse and does
//! nothing else, so Windows always gets its answer in time; a second one moves the window.

use crate::config::{HAnchor, VAnchor};
use crate::i18n::tr;
use crate::platform;
use crate::styles::NativeCorner;
use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU16, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::System::Threading::*;
use windows::Win32::System::Variant::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Opened {
    Menu,
    Search,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Command {
    Open(Opened),
    Customize,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Placement {
    pub h: HAnchor,
    pub v: VAnchor,
    pub width: f32,
    pub height: f32,
    pub margin: f32,
    /// A menu floats above everything; the settings window is a normal window.
    pub on_top: bool,
}

/// The outline of the window itself.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Shape {
    /// Corner size in points at the app's own zoom. Zero is square.
    pub corner: f32,
    pub round_bottom: bool,
    /// Windows rounds the window. Otherwise our shape cuts the window to size.
    pub native: Option<NativeCorner>,
}

/// Everything shared between the menu and the system thread.
pub struct Shared {
    hwnd: AtomicIsize,
    pub visible: AtomicBool,
    pub take_start: AtomicBool,
    pub take_search: AtomicBool,
    /// While a dialog of our own is open, losing focus must not close the menu.
    pub hold_open: AtomicBool,
    ctx: OnceLock<egui::Context>,
    commands: Mutex<VecDeque<Command>>,
    placement: Mutex<Placement>,
    start_buttons: Mutex<Vec<RECT>>,
    native_until: Mutex<Option<Instant>>,
    shape: Mutex<Shape>,
    hidden_at: Mutex<Option<Instant>>,
    /// Bumped by every open and close, so an older fade knows to stop.
    fade_generation: AtomicU64,
    /// The window that had the focus before the menu opened.
    previous_foreground: AtomicIsize,
    pub accept_injected: AtomicBool,
    /// Policies can fix the Start key setting and remove things from the tray menu.
    pub tray_fixed_start: AtomicBool,
    pub tray_no_settings: AtomicBool,
    pub tray_no_native: AtomicBool,
    pub tray_no_quit: AtomicBool,
}

impl Shared {
    pub fn new(placement: Placement, take_start: bool, take_search: bool) -> Arc<Shared> {
        Arc::new(Shared {
            hwnd: AtomicIsize::new(0),
            visible: AtomicBool::new(true),
            take_start: AtomicBool::new(take_start),
            take_search: AtomicBool::new(take_search),
            hold_open: AtomicBool::new(false),
            ctx: OnceLock::new(),
            commands: Mutex::new(VecDeque::new()),
            placement: Mutex::new(placement),
            start_buttons: Mutex::new(Vec::new()),
            native_until: Mutex::new(None),
            shape: Mutex::new(Shape { corner: 0.0, round_bottom: true, native: None }),
            hidden_at: Mutex::new(None),
            fade_generation: AtomicU64::new(0),
            previous_foreground: AtomicIsize::new(0),
            accept_injected: AtomicBool::new(false),
            tray_fixed_start: AtomicBool::new(false),
            tray_no_settings: AtomicBool::new(false),
            tray_no_native: AtomicBool::new(false),
            tray_no_quit: AtomicBool::new(false),
        })
    }

    /// The window's handle as a plain number, for dialogs to open in front of it.
    pub fn window_id(&self) -> isize {
        self.hwnd.load(Ordering::SeqCst)
    }

    pub fn attach(&self, hwnd: isize, ctx: egui::Context) {
        stop_raw_keyboard();
        self.hwnd.store(hwnd, Ordering::SeqCst);
        let _ = self.ctx.set(ctx);
    }

    fn window(&self) -> Option<HWND> {
        match self.hwnd.load(Ordering::SeqCst) {
            0 => None,
            raw => Some(HWND(raw as *mut c_void)),
        }
    }

    pub fn take_commands(&self) -> Vec<Command> {
        self.commands.lock().unwrap().drain(..).collect()
    }

    pub fn push(&self, command: Command) {
        self.commands.lock().unwrap().push_back(command);
        if let Some(ctx) = self.ctx.get() {
            ctx.request_repaint();
        }
    }

    pub fn set_placement(&self, placement: Placement) {
        let changed = {
            let mut current = self.placement.lock().unwrap();
            let changed = *current != placement;
            *current = placement;
            changed
        };
        if changed && self.visible.load(Ordering::SeqCst) {
            self.place_window(false);
        }
    }

    /// Moves the window to its spot on the monitor where the pointer is.
    pub fn place_window(&self, show: bool) {
        let Some(hwnd) = self.window() else { return };
        let placement = *self.placement.lock().unwrap();
        unsafe {
            let mut cursor = POINT::default();
            let _ = GetCursorPos(&mut cursor);
            let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTOPRIMARY);
            let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return;
            }
            let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
            let scale = dpi_x as f32 / 96.0;
            let area = info.rcWork;
            let (area_w, area_h) = (area.right - area.left, area.bottom - area.top);
            let margin = (placement.margin * scale) as i32;
            let w = ((placement.width * scale) as i32).min(area_w - margin * 2);
            let h = ((placement.height * scale) as i32).min(area_h - margin * 2);
            let x = match placement.h {
                HAnchor::Left => area.left + margin,
                HAnchor::Center => area.left + (area_w - w) / 2,
                HAnchor::Right => area.right - w - margin,
                HAnchor::FollowStart => {
                    let button = self.start_buttons.lock().unwrap().iter().find(|r| r.left >= area.left && r.right <= area.right).copied();
                    button.map_or(area.left, |r| r.left).clamp(area.left, (area.right - w).max(area.left))
                }
            };
            let y = match placement.v {
                VAnchor::Bottom => area.bottom - h - margin,
                VAnchor::Middle => area.top + (area_h - h) / 2,
                VAnchor::Top => area.top + margin,
            };
            let flags = if show { SWP_SHOWWINDOW } else { SWP_NOACTIVATE };
            let layer = if placement.on_top { HWND_TOPMOST } else { HWND_NOTOPMOST };
            let _ = SetWindowPos(hwnd, Some(layer), x, y, w, h, flags);
            cut_to_shape(hwnd, *self.shape.lock().unwrap(), w, h, scale);
        }
    }

    /// Changes the window's outline. Placing the window cuts it again at any new size.
    pub fn set_shape(&self, shape: Shape) {
        *self.shape.lock().unwrap() = shape;
        let Some(hwnd) = self.window() else { return };
        set_window_corners(hwnd, shape.native);
        self.place_window(false);
    }

    pub fn open(&self, how: Opened) {
        let Some(hwnd) = self.window() else { return };
        let before = unsafe { GetForegroundWindow() };
        if before != hwnd && !before.0.is_null() {
            self.previous_foreground.store(before.0 as isize, Ordering::SeqCst);
        }
        // A fade still running stops here, and the window comes back fully solid.
        self.fade_generation.fetch_add(1, Ordering::SeqCst);
        set_window_alpha(hwnd, None);
        self.visible.store(true, Ordering::SeqCst);
        self.push(Command::Open(how));
        let started = Instant::now();
        self.place_window(true);
        let placed = started.elapsed();
        force_foreground(hwnd);
        log_if_testing(self, &format!("opened: placing took {placed:?}, focus took {:?}", started.elapsed() - placed)); // i18n-ignore: test log
        if let Some(ctx) = self.ctx.get() {
            ctx.request_repaint();
        }
    }

    /// Closes the menu with a short fade and hands the focus back to what had it before.
    pub fn hide(self: &Arc<Self>) {
        self.fade_out(true);
    }

    /// The same when something was just launched: its new window should get the focus.
    pub fn hide_for_launch(self: &Arc<Self>) {
        self.fade_out(false);
    }

    fn fade_out(self: &Arc<Self>, give_focus_back: bool) {
        let Some(hwnd) = self.window() else { return };
        if !self.visible.swap(false, Ordering::SeqCst) {
            return;
        }
        if give_focus_back && unsafe { GetForegroundWindow() } == hwnd {
            let previous = HWND(self.previous_foreground.load(Ordering::SeqCst) as *mut c_void);
            if !previous.0.is_null() && unsafe { IsWindow(Some(previous)) }.as_bool() {
                let _ = unsafe { SetForegroundWindow(previous) };
            }
        }
        *self.hidden_at.lock().unwrap() = Some(Instant::now());
        let generation = self.fade_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let raw = hwnd.0 as isize;
        let shared = self.clone();
        std::thread::spawn(move || {
            let hwnd = HWND(raw as *mut c_void);
            let started = Instant::now();
            loop {
                if shared.fade_generation.load(Ordering::SeqCst) != generation {
                    return;
                }
                let done = started.elapsed().as_secs_f32() / FADE_SECONDS;
                if done >= 1.0 {
                    break;
                }
                // Starts gently and speeds up, which reads as smooth at this short length.
                set_window_alpha(hwnd, Some((255.0 * (1.0 - done * done)) as u8));
                std::thread::sleep(Duration::from_millis(8));
            }
            let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
            set_window_alpha(hwnd, None);
        });
    }

    /// Closes the menu at once, for when something else is taking its place right away.
    pub fn hide_now(&self) {
        let Some(hwnd) = self.window() else { return };
        self.visible.store(false, Ordering::SeqCst);
        *self.hidden_at.lock().unwrap() = Some(Instant::now());
        self.fade_generation.fetch_add(1, Ordering::SeqCst);
        let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
        set_window_alpha(hwnd, None);
    }

    pub fn toggle(self: &Arc<Self>, how: Opened) {
        let is_foreground = self.window().is_some_and(|w| unsafe { GetForegroundWindow() } == w);
        log_if_testing(self, &format!("toggle visible={} foreground={is_foreground}", self.visible.load(Ordering::SeqCst))); // i18n-ignore: test log
        if self.visible.load(Ordering::SeqCst) && is_foreground {
            self.hide();
        } else {
            self.open(how);
        }
    }

    /// Lets the real Start menu through once.
    pub fn open_native_start(&self) {
        *self.native_until.lock().unwrap() = Some(Instant::now() + Duration::from_millis(1500));
        self.hide_now();
        // Windows ignores the Start key while Shift is down, so let go of it first.
        let mut keys: Vec<(u16, bool)> = Vec::new();
        for shift in [VK_LSHIFT.0, VK_RSHIFT.0] {
            if unsafe { GetAsyncKeyState(shift as i32) } as u16 & 0x8000 != 0 {
                keys.push((shift, true));
            }
        }
        keys.extend([(VK_LWIN.0, false), (VK_LWIN.0, true)]);
        press_keys(&keys);
    }

    /// Frosted glass behind the window, where Windows allows it.
    pub fn set_blur(&self, on: bool, tint: [u8; 3], alpha: f32) {
        let Some(hwnd) = self.window() else { return };
        set_blur(hwnd, on, tint, alpha);
    }

    /// Windows 11's Mica behind the window, tinted for a dark or light theme, or none.
    pub fn set_mica(&self, on: bool, dark: bool) {
        let Some(hwnd) = self.window() else { return };
        set_backdrop(hwnd, on, dark);
    }

    /// The menu closed a moment ago, so a Windows menu showing up now is not a request to open it.
    fn just_hidden(&self) -> bool {
        self.hidden_at.lock().unwrap().is_some_and(|at| at.elapsed() < Duration::from_millis(900))
    }

    fn native_allowed(&self) -> bool {
        self.native_until.lock().unwrap().is_some_and(|until| Instant::now() < until)
    }
}

/// The window library signs up for raw keyboard input, which nothing here uses. While it is
/// on, Windows stops calling the keyboard watcher once this window has had the focus, until
/// another program takes it: keys then go straight past, and a Start key press opens the
/// real menu. Seen in test runs and gone with this; see shots/test-start-key.ps1.
fn stop_raw_keyboard() {
    use windows::Win32::UI::Input::{RAWINPUTDEVICE, RIDEV_REMOVE, RegisterRawInputDevices};
    const GENERIC_DESKTOP: u16 = 1;
    const KEYBOARD: u16 = 6;
    let keyboard = RAWINPUTDEVICE { usUsagePage: GENERIC_DESKTOP, usUsage: KEYBOARD, dwFlags: RIDEV_REMOVE, hwndTarget: HWND::default() };
    let _ = unsafe { RegisterRawInputDevices(&[keyboard], std::mem::size_of::<RAWINPUTDEVICE>() as u32) };
}

fn force_foreground(hwnd: HWND) {
    unsafe {
        // Windows only lets the app with the newest input take focus. Tapping a harmless key
        // makes that us, and briefly sharing input with the current window covers the rest.
        press_keys(&[(MASK_KEY, false), (MASK_KEY, true)]);
        let foreground = GetForegroundWindow();
        let their_thread = GetWindowThreadProcessId(foreground, None);
        let my_thread = GetCurrentThreadId();
        let attached = their_thread != 0 && their_thread != my_thread && AttachThreadInput(my_thread, their_thread, true).as_bool();
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
        if attached {
            let _ = AttachThreadInput(my_thread, their_thread, false);
        }
    }
}

/// A key code Windows leaves unassigned. Tapping it makes this program the one with the
/// newest input, which Windows requires before it lets a window take focus.
const MASK_KEY: u16 = 0xE8;

/// Marks key presses this program sends, so its own keyboard watcher can tell them apart.
const OWN_KEYS: usize = 0x5354_4C4E;

const FADE_SECONDS: f32 = 0.14;

/// Sets how see-through the whole window is. `None` is the normal, solid window.
fn set_window_alpha(hwnd: HWND, alpha: Option<u8>) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let layered = WS_EX_LAYERED.0 as isize;
        match alpha {
            Some(alpha) => {
                if style & layered == 0 {
                    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | layered);
                }
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
            }
            None => {
                if style & layered != 0 {
                    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style & !layered);
                }
            }
        }
    }
}

fn stroke(vk: u16, up: bool, scan: u16, extended: bool) -> INPUT {
    let mut flags = if up { KEYEVENTF_KEYUP } else { Default::default() };
    if extended || vk == VK_LWIN.0 || vk == VK_RWIN.0 {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), wScan: scan, dwFlags: flags, dwExtraInfo: OWN_KEYS, ..Default::default() },
        },
    }
}

fn send_strokes(inputs: &[INPUT]) {
    unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
}

fn press_keys(keys: &[(u16, bool)]) {
    let inputs: Vec<INPUT> = keys.iter().map(|&(vk, up)| stroke(vk, up, 0, false)).collect();
    send_strokes(&inputs);
}

// ---- The Start key decision, kept free of Windows calls so it can be tested ----

/// The Start key is held back from Windows the moment it goes down. Windows only ever hears
/// about it when it turns out to be part of a shortcut, so the real menu has nothing to
/// react to. Covering the press up afterwards is not reliable: whether Windows opens its
/// menu depends on timing, and it did open whenever this menu had the focus.
#[derive(Default)]
pub struct WinKeyMachine {
    win_down: bool,
    /// Windows was told the Start key is down, so its release belongs to Windows too.
    told_windows: bool,
    /// Shift was held: the person asked for the real Start menu.
    native: bool,
    /// The press already did its job, so the release should do nothing more.
    consumed: bool,
    swallow_up: Option<u16>,
    last_win_event: u32,
}

#[derive(Default, Debug, PartialEq)]
pub struct KeyDecision {
    /// Keep this key event from reaching Windows.
    pub swallow: bool,
    /// Tell Windows the Start key went down, then press this key: it is a shortcut.
    pub shortcut_key: Option<u16>,
    /// Hand Windows the whole Start key press: this menu isn't taking it over.
    pub replay_press: bool,
    pub open: Option<Opened>,
    pub open_native: bool,
    pub lock: bool,
}

const VK_S: u16 = 0x53;
const VK_L: u16 = 0x4C;

/// Longer than this without any Start key event, and its release was missed (say, it was let
/// go on the lock screen). A key held down keeps repeating well within this time.
const WIN_KEY_STALE_MS: u32 = 2000;

fn is_modifier(vk: u16) -> bool {
    [VK_SHIFT, VK_LSHIFT, VK_RSHIFT, VK_CONTROL, VK_LCONTROL, VK_RCONTROL, VK_MENU, VK_LMENU, VK_RMENU].iter().any(|k| k.0 == vk)
}

impl WinKeyMachine {
    #[allow(clippy::too_many_arguments)]
    pub fn on_key(&mut self, vk: u16, down: bool, shift: bool, ctrl_or_alt: bool, take_start: bool, take_search: bool, time: u32) -> KeyDecision {
        let mut decision = KeyDecision::default();
        if vk == VK_LWIN.0 || vk == VK_RWIN.0 {
            if down {
                let new_press = !self.win_down || time.wrapping_sub(self.last_win_event) > WIN_KEY_STALE_MS;
                self.last_win_event = time;
                if new_press {
                    self.win_down = true;
                    self.consumed = false;
                    self.native = shift && !ctrl_or_alt;
                    // Ctrl or Alt first means a shortcut, such as switching desktops.
                    self.told_windows = ctrl_or_alt;
                }
                decision.swallow = !self.told_windows;
            } else {
                let was_down = std::mem::take(&mut self.win_down);
                if was_down && !self.told_windows {
                    decision.swallow = true;
                    if self.consumed {
                        // Start + S or Start + L already did what was wanted.
                    } else if self.native || shift {
                        decision.open_native = true;
                    } else if take_start {
                        decision.open = Some(Opened::Menu);
                    } else {
                        decision.replay_press = true;
                    }
                }
            }
            return decision;
        }
        if !down && self.swallow_up == Some(vk) {
            self.swallow_up = None;
            decision.swallow = true;
            return decision;
        }
        if self.win_down && !self.told_windows && down {
            if time.wrapping_sub(self.last_win_event) > WIN_KEY_STALE_MS {
                // Its release was missed. This is ordinary typing, not a shortcut.
                self.win_down = false;
                return decision;
            }
            if is_modifier(vk) {
                return decision;
            }
            let plain = !shift && !ctrl_or_alt;
            decision.swallow = true;
            if vk == VK_S && take_search && plain {
                self.consumed = true;
                self.swallow_up = Some(vk);
                decision.open = Some(Opened::Search);
            } else if vk == VK_L && plain {
                // Windows reads Start + L from the keyboard itself and ignores a simulated one.
                self.consumed = true;
                self.swallow_up = Some(vk);
                decision.lock = true;
            } else {
                self.told_windows = true;
                decision.shortcut_key = Some(vk);
            }
        }
        decision
    }

    /// A mouse button or wheel used with Start, as in Start + scroll. True if Windows should
    /// now be told the Start key is down.
    pub fn on_mouse(&mut self) -> bool {
        if self.win_down && !self.told_windows && !self.consumed {
            self.told_windows = true;
            return true;
        }
        false
    }
}

// ---- Hooks ----

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
static MACHINE: Mutex<Option<WinKeyMachine>> = Mutex::new(None);
static START_CLICK_SWALLOWED: AtomicBool = AtomicBool::new(false);
static LAST_WIN_KEY: AtomicU16 = AtomicU16::new(VK_LWIN.0);
static SYSTEM_WINDOW: AtomicIsize = AtomicIsize::new(0);

const WM_APP_OPEN_MENU: u32 = WM_APP + 1;
const WM_APP_TRAY: u32 = WM_APP + 3;
const WM_APP_QUIT: u32 = WM_APP + 4;

/// During a test run, notes what happened and when, for comparing with what the screen did.
fn test_log(line: &str) {
    use std::io::Write;
    static STARTED: OnceLock<Instant> = OnceLock::new();
    let at = STARTED.get_or_init(Instant::now).elapsed().as_millis();
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(std::env::temp_dir().join("startline-test.log")) {
        let _ = writeln!(file, "{at:>7} {line}");
    }
}

fn log_if_testing(shared: &Shared, line: &str) {
    if shared.accept_injected.load(Ordering::Relaxed) {
        test_log(line);
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if code as u32 == HC_ACTION {
            let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let shared = SHARED.get();
            let injected = info.flags.0 & LLKHF_INJECTED.0 != 0;
            let accept_injected = shared.is_some_and(|s| s.accept_injected.load(Ordering::Relaxed));
            let own_keys = info.dwExtraInfo == OWN_KEYS;
            if let (Some(shared), true) = (shared, !own_keys && (!injected || accept_injected)) {
                let take_start = shared.take_start.load(Ordering::Relaxed);
                let take_search = shared.take_search.load(Ordering::Relaxed);
                if take_start || take_search {
                    let down = wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN;
                    let held = |vk: VIRTUAL_KEY| GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0;
                    let vk = info.vkCode as u16;
                    let decision = MACHINE.lock().unwrap().get_or_insert_with(WinKeyMachine::default).on_key(
                        vk,
                        down,
                        held(VK_SHIFT),
                        held(VK_CONTROL) || held(VK_MENU),
                        take_start,
                        take_search,
                        info.time,
                    );
                    if accept_injected && (vk == VK_LWIN.0 || vk == VK_RWIN.0) {
                        test_log(&format!(
                            "key {vk:#x} down={down} injected={injected} shift={} ctrl_alt={} -> {decision:?}",
                            held(VK_SHIFT),
                            held(VK_CONTROL) || held(VK_MENU)
                        ));
                    }
                    if let Some(key) = decision.shortcut_key {
                        let win = LAST_WIN_KEY.load(Ordering::Relaxed);
                        let extended = info.flags.0 & LLKHF_EXTENDED.0 != 0;
                        send_strokes(&[stroke(win, false, 0, false), stroke(key, false, info.scanCode as u16, extended)]);
                    }
                    if vk == VK_LWIN.0 || vk == VK_RWIN.0 {
                        LAST_WIN_KEY.store(vk, Ordering::Relaxed);
                    }
                    if decision.replay_press {
                        press_keys(&[(vk, false), (vk, true)]);
                    }
                    if decision.open_native {
                        send_job(Job::OpenNative);
                    }
                    if decision.lock {
                        send_job(Job::Lock);
                    }
                    match decision.open {
                        Some(Opened::Menu) => send_job(Job::Toggle),
                        Some(Opened::Search) => send_job(Job::Open),
                        None => {}
                    }
                    if decision.swallow {
                        return LRESULT(1);
                    }
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if code as u32 == HC_ACTION {
            let message = wparam.0 as u32;
            if matches!(message, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_MOUSEWHEEL | WM_MOUSEHWHEEL)
                && MACHINE.lock().unwrap().as_mut().is_some_and(|machine| machine.on_mouse())
            {
                press_keys(&[(LAST_WIN_KEY.load(Ordering::Relaxed), false)]);
            }
            if message == WM_LBUTTONDOWN || message == WM_LBUTTONUP {
                let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
                if let Some(shared) = SHARED.get() {
                    if shared.take_start.load(Ordering::Relaxed) {
                        if message == WM_LBUTTONDOWN {
                            let hit = shared.start_buttons.lock().unwrap().iter().any(|r| {
                                info.pt.x >= r.left && info.pt.x < r.right && info.pt.y >= r.top && info.pt.y < r.bottom
                            });
                            if hit {
                                START_CLICK_SWALLOWED.store(true, Ordering::SeqCst);
                                return LRESULT(1);
                            }
                        } else if START_CLICK_SWALLOWED.swap(false, Ordering::SeqCst) {
                            send_job(Job::Toggle);
                            return LRESULT(1);
                        }
                    }
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }
}

fn process_name(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 512];
        let mut size = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut size);
        let _ = CloseHandle(process);
        result.ok()?;
        let path = String::from_utf16_lossy(&buffer[..size as usize]);
        path.rsplit('\\').next().map(|s| s.to_ascii_lowercase())
    }
}

/// Catches every other way the real menus can open (Ctrl+Esc, a touch swipe, a stale
/// button position), and closes our menu when the person moves on to something else.
unsafe extern "system" fn foreground_changed(_: HWINEVENTHOOK, _: u32, hwnd: HWND, _: i32, _: i32, _: u32, _: u32) {
    send_job(Job::Foreground(hwnd.0 as isize));
}

/// Work for the window thread. The thread that watches the keyboard must never do any of
/// this itself: if it is busy when a key comes, Windows stops waiting for its answer and
/// hands the key straight on.
enum Job {
    Toggle,
    Open,
    OpenNative,
    Lock,
    Foreground(isize),
}

static JOBS: OnceLock<Sender<Job>> = OnceLock::new();

fn send_job(job: Job) {
    if let Some(jobs) = JOBS.get() {
        let _ = jobs.send(job);
    }
}

fn window_thread(shared: Arc<Shared>, jobs: Receiver<Job>) {
    for job in jobs {
        let started = Instant::now();
        match job {
            Job::Toggle => shared.toggle(Opened::Menu),
            Job::Open => shared.open(Opened::Search),
            Job::OpenNative => shared.open_native_start(),
            Job::Lock => {
                let _ = unsafe { windows::Win32::System::Shutdown::LockWorkStation() };
            }
            Job::Foreground(raw) => on_foreground(&shared, HWND(raw as *mut c_void)),
        }
        if started.elapsed() > Duration::from_millis(100) {
            log_if_testing(&shared, &format!("a window job took {:?}", started.elapsed())); // i18n-ignore: test log
        }
    }
}

fn on_foreground(shared: &Arc<Shared>, hwnd: HWND) {
    let ours = shared.window();
    if ours == Some(hwnd) || hwnd.0.is_null() {
        return;
    }
    let name = process_name(hwnd).unwrap_or_default();
    log_if_testing(shared, &format!("foreground -> {name} (menu visible={})", shared.visible.load(Ordering::SeqCst))); // i18n-ignore: test log
    let native_start = name == "startmenuexperiencehost.exe";
    let native_search = name == "searchhost.exe" || name == "searchapp.exe";
    if (native_start && shared.take_start.load(Ordering::Relaxed) || native_search && shared.take_search.load(Ordering::Relaxed))
        && !shared.native_allowed()
    {
        // Our menu has search built in, so this is always the same menu, and a press
        // that just closed it must leave it closed.
        press_keys(&[(VK_ESCAPE.0, false), (VK_ESCAPE.0, true)]);
        if !shared.just_hidden() {
            shared.open(Opened::Menu);
        }
        return;
    }
    if shared.visible.load(Ordering::SeqCst) && !shared.hold_open.load(Ordering::SeqCst) {
        shared.hide();
    }
}

unsafe extern "system" fn system_wndproc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        let shared = SHARED.get();
        match message {
            // Sent by a second launch of the program.
            WM_APP_OPEN_MENU => {
                send_job(Job::Toggle);
                LRESULT(0)
            }
            WM_APP_QUIT => shutdown(),
            WM_APP_TRAY => {
                match lparam.0 as u32 & 0xFFFF {
                    WM_LBUTTONUP => send_job(Job::Toggle),
                    WM_RBUTTONUP | WM_CONTEXTMENU => {
                        if let Some(s) = shared { show_tray_menu(hwnd, s); }
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                remove_tray(hwnd);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

const TRAY_ID: u32 = 7;
const MENU_OPEN: usize = 1;
const MENU_CUSTOMIZE: usize = 2;
const MENU_TAKE_OVER: usize = 3;
const MENU_NATIVE: usize = 4;
const MENU_QUIT: usize = 5;

/// A menu item in the language shown now. The tray menu is built each time it opens, so it follows the language.
fn append_item(menu: HMENU, flags: MENU_ITEM_FLAGS, id: usize, text: &str) {
    let text = platform::wide(text);
    let _ = unsafe { AppendMenuW(menu, flags, id, PCWSTR(text.as_ptr())) };
}

unsafe fn show_tray_menu(hwnd: HWND, shared: &Arc<Shared>) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let taking = shared.take_start.load(Ordering::Relaxed);
        append_item(menu, MF_STRING, MENU_OPEN, tr("Open Startline"));
        if !shared.tray_no_settings.load(Ordering::Relaxed) {
            append_item(menu, MF_STRING, MENU_CUSTOMIZE, tr("Settings..."));
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let check = if taking { MF_CHECKED } else { MF_UNCHECKED };
        let fixed = if shared.tray_fixed_start.load(Ordering::Relaxed) { MF_GRAYED } else { MF_ENABLED };
        append_item(menu, MF_STRING | check | fixed, MENU_TAKE_OVER, tr("Start key opens Startline"));
        if !shared.tray_no_native.load(Ordering::Relaxed) {
            append_item(menu, MF_STRING, MENU_NATIVE, tr("Open the Windows Start menu"));
        }
        if !shared.tray_no_quit.load(Ordering::Relaxed) {
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            append_item(menu, MF_STRING, MENU_QUIT, tr("Quit Startline"));
        }
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let _ = SetForegroundWindow(hwnd);
        let picked = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_BOTTOMALIGN, point.x, point.y, None, hwnd, None);
        let _ = DestroyMenu(menu);
        match picked.0 as usize {
            MENU_OPEN => send_job(Job::Open),
            MENU_CUSTOMIZE => {
                send_job(Job::Open);
                shared.push(Command::Customize);
            }
            MENU_TAKE_OVER => {
                shared.take_start.store(!taking, Ordering::Relaxed);
                shared.take_search.store(!taking, Ordering::Relaxed);
                if let Some(ctx) = shared.ctx.get() {
                    ctx.request_repaint();
                }
            }
            MENU_NATIVE => send_job(Job::OpenNative),
            MENU_QUIT => {
                let _ = PostMessageW(Some(hwnd), WM_APP_QUIT, WPARAM(0), LPARAM(0));
            }
            _ => {}
        }
    }
}

fn remove_tray(hwnd: HWND) {
    let data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        ..Default::default()
    };
    let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
}

fn add_tray(hwnd: HWND) {
    let mut data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_APP_TRAY,
        hIcon: make_tray_icon(),
        ..Default::default()
    };
    let tip = platform::wide("Startline"); // i18n-ignore: the app's name
    data.szTip[..tip.len()].copy_from_slice(&tip);
    let _ = unsafe { Shell_NotifyIconW(NIM_ADD, &data) };
}

fn make_tray_icon() -> HICON {
    const SIZE: usize = 32;
    let rgba = crate::app_icon_pixels(SIZE);
    // Windows wants blue-green-red order here.
    let bgra: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
    let mask = vec![0u8; SIZE * SIZE / 8];
    unsafe {
        let color = CreateBitmap(SIZE as i32, SIZE as i32, 1, 32, Some(bgra.as_ptr() as *const c_void));
        let mono = CreateBitmap(SIZE as i32, SIZE as i32, 1, 1, Some(mask.as_ptr() as *const c_void));
        let info = ICONINFO { fIcon: TRUE, xHotspot: 0, yHotspot: 0, hbmMask: mono, hbmColor: color };
        CreateIconIndirect(&info).unwrap_or_default()
    }
}

/// Keeps the Start button positions current. On a centered taskbar the button moves as
/// windows open, so it looks often while the pointer is on the taskbar and rarely otherwise.
fn watch_start_buttons(shared: Arc<Shared>) {
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let Ok(automation) = (unsafe { CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }) else {
        return;
    };
    let mut last_full = Instant::now() - Duration::from_secs(3600);
    loop {
        std::thread::sleep(Duration::from_millis(300));
        if !shared.take_start.load(Ordering::Relaxed) {
            continue;
        }
        let bars = taskbars();
        let mut cursor = POINT::default();
        let _ = unsafe { GetCursorPos(&mut cursor) };
        let hovering = bars.iter().any(|bar| {
            let mut rect = RECT::default();
            let _ = unsafe { GetWindowRect(*bar, &mut rect) };
            cursor.x >= rect.left && cursor.x < rect.right && cursor.y >= rect.top && cursor.y < rect.bottom
        });
        if hovering || last_full.elapsed() > Duration::from_secs(5) {
            last_full = Instant::now();
            let rects = find_start_buttons(&automation, &bars);
            *shared.start_buttons.lock().unwrap() = rects;
        }
    }
}

fn taskbars() -> Vec<HWND> {
    let mut out = Vec::new();
    unsafe {
        if let Ok(main) = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) {
            out.push(main);
        }
        let mut after = None;
        while let Ok(bar) = FindWindowExW(None, after, w!("Shell_SecondaryTrayWnd"), PCWSTR::null()) {
            out.push(bar);
            after = Some(bar);
        }
    }
    out
}

pub fn find_start_buttons(automation: &IUIAutomation, bars: &[HWND]) -> Vec<RECT> {
    unsafe {
        let Ok(condition) = automation.CreatePropertyCondition(UIA_AutomationIdPropertyId, &VARIANT::from(BSTR::from("StartButton"))) else {
            return Vec::new();
        };
        bars.iter()
            .filter_map(|bar| {
                let element = automation.ElementFromHandle(*bar).ok()?;
                let button = element.FindFirst(TreeScope_Descendants, &condition).ok()?;
                let rect = button.CurrentBoundingRectangle().ok()?;
                (rect.right > rect.left && rect.bottom > rect.top).then_some(rect)
            })
            .collect()
    }
}

/// Starts the system thread. Without `hooks` there is only the tray icon.
pub fn start(shared: Arc<Shared>, hooks: bool) {
    let _ = SHARED.set(shared.clone());
    let (jobs, incoming) = mpsc::channel();
    let _ = JOBS.set(jobs);
    let worker = shared.clone();
    let _ = std::thread::Builder::new().name("window".into()).spawn(move || window_thread(worker, incoming));
    if hooks {
        let watcher = shared.clone();
        let _ = std::thread::Builder::new().name("start-button".into()).spawn(move || watch_start_buttons(watcher));
    }
    std::thread::Builder::new()
        .name("system".into())
        .spawn(move || unsafe {
            // The keyboard must never lag because the machine is busy.
            let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
            platform::com_init();
            let instance = GetModuleHandleW(None).map(|m| HINSTANCE(m.0)).unwrap_or_default();
            let class = WNDCLASSW {
                lpfnWndProc: Some(system_wndproc),
                hInstance: instance,
                lpszClassName: SYSTEM_CLASS,
                ..Default::default()
            };
            RegisterClassW(&class);
            let Ok(window) = CreateWindowExW(
                Default::default(),
                SYSTEM_CLASS,
                w!("Startline system"),
                WS_OVERLAPPED,
                0, 0, 0, 0,
                None, None, Some(instance), None,
            ) else {
                return;
            };
            SYSTEM_WINDOW.store(window.0 as isize, Ordering::SeqCst);
            add_tray(window);
            if hooks {
                let _ = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), Some(instance), 0);
                let _ = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), Some(instance), 0);
            }
            let _ = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(foreground_changed),
                0,
                0,
                // Our own windows count too: clicking the settings window closes the menu.
                WINEVENT_OUTOFCONTEXT,
            );
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        })
        .expect("system thread");
}

const SYSTEM_CLASS: PCWSTR = w!("StartlineSystemWindow");

/// If Startline already runs, asks it to open and returns true.
pub fn signal_running_instance() -> bool {
    unsafe {
        let Ok(window) = FindWindowW(SYSTEM_CLASS, PCWSTR::null()) else { return false };
        let mut pid = 0;
        GetWindowThreadProcessId(window, Some(&mut pid));
        let _ = AllowSetForegroundWindow(pid);
        let _ = PostMessageW(Some(window), WM_APP_OPEN_MENU, WPARAM(0), LPARAM(0));
        true
    }
}

/// Ends the program. Windows drops the keyboard connection along with the process.
pub fn shutdown() -> ! {
    let hwnd = SYSTEM_WINDOW.load(Ordering::SeqCst);
    if hwnd != 0 {
        remove_tray(HWND(hwnd as *mut c_void));
    }
    std::process::exit(0)
}

/// Reports what the Start button search finds, for checking on a real machine.
pub fn probe_start_button() -> String {
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let Ok(automation) = (unsafe { CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }) else {
        return "start button: automation unavailable\n".into(); // i18n-ignore: probe report for tests
    };
    let bars = taskbars();
    let started = Instant::now();
    let rects = find_start_buttons(&automation, &bars);
    let mut out = format!("taskbars={} start buttons found={} in {:?}\n", bars.len(), rects.len(), started.elapsed()); // i18n-ignore: probe report for tests
    for rect in rects {
        out.push_str(&format!("  start button at left={} top={} right={} bottom={}\n", rect.left, rect.top, rect.right, rect.bottom)); // i18n-ignore: probe report for tests
    }
    out
}

// ---- Frosted glass ----

/// Asks Windows for its rounded window, or for a plain one with no border of its own.
fn set_window_corners(hwnd: HWND, native: Option<NativeCorner>) {
    const COLOR_NONE: u32 = 0xFFFF_FFFE;
    const COLOR_DEFAULT: u32 = 0xFFFF_FFFF;
    let corner = match native {
        Some(NativeCorner::Round) => DWMWCP_ROUND,
        Some(NativeCorner::Small) => DWMWCP_ROUNDSMALL,
        None => DWMWCP_DONOTROUND,
    };
    let border = if native.is_some() { COLOR_DEFAULT } else { COLOR_NONE };
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corner as *const _ as *const c_void, std::mem::size_of_val(&corner) as u32);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &border as *const _ as *const c_void, std::mem::size_of_val(&border) as u32);
    }
}

/// Cuts the window, glass included, to the outline the app paints.
fn cut_to_shape(hwnd: HWND, shape: Shape, width: i32, height: i32, scale: f32) {
    unsafe {
        if shape.native.is_some() || shape.corner <= 0.0 {
            SetWindowRgn(hwnd, None, true);
            return;
        }
        let diameter = (shape.corner * scale * 2.0).round() as i32;
        // Reaching past the bottom edge leaves the lower corners square.
        let bottom = if shape.round_bottom { height + 1 } else { height + diameter };
        // The top row of a frameless window is a pale strip of frame, above where the app
        // paints. Starting one row down leaves it out and lines the corners up with the paint.
        let region = CreateRoundRectRgn(0, 1, width + 1, bottom, diameter, diameter);
        SetWindowRgn(hwnd, Some(region), true);
    }
}

/// Mica, or none. Mica Alt, as Explorer uses behind its tabs: more of the desktop picture's color comes through than
/// plain Mica's. Mica only shows where the window counts as frame to Windows, so the frame is stretched over all of
/// it. Stretched from the bottom rather than the top, so Windows doesn't draw caption buttons of its own there.
fn set_backdrop(hwnd: HWND, on: bool, dark: bool) {
    use windows::Win32::UI::Controls::MARGINS;
    let dark = BOOL::from(dark);
    let kind = if on { DWMSBT_TABBEDWINDOW } else { DWMSBT_NONE };
    let up = if on { 32_000 } else { 0 };
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as *const c_void, 4);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &kind as *const _ as *const c_void, 4);
        let _ = DwmExtendFrameIntoClientArea(hwnd, &MARGINS { cxLeftWidth: 0, cxRightWidth: 0, cyTopHeight: 0, cyBottomHeight: up });
    }
}

fn set_blur(hwnd: HWND, on: bool, tint: [u8; 3], alpha: f32) {
    #[repr(C)]
    struct AccentPolicy {
        state: u32,
        flags: u32,
        gradient: u32,
        animation: u32,
    }
    #[repr(C)]
    struct CompositionData {
        attribute: u32,
        data: *mut c_void,
        size: usize,
    }
    const WCA_ACCENT_POLICY: u32 = 19;
    const ACCENT_DISABLED: u32 = 0;
    const ACCENT_ACRYLIC: u32 = 4;
    unsafe {
        let Ok(user32) = GetModuleHandleW(w!("user32.dll")) else { return };
        let Some(raw) = GetProcAddress(user32, s!("SetWindowCompositionAttribute")) else { return };
        let call: unsafe extern "system" fn(HWND, *mut CompositionData) -> BOOL = std::mem::transmute(raw);
        let gradient = ((alpha.clamp(0.0, 1.0) * 255.0) as u32) << 24
            | (tint[2] as u32) << 16
            | (tint[1] as u32) << 8
            | tint[0] as u32;
        let mut policy = AccentPolicy { state: if on { ACCENT_ACRYLIC } else { ACCENT_DISABLED }, flags: 2, gradient, animation: 0 };
        let mut data = CompositionData {
            attribute: WCA_ACCENT_POLICY,
            data: &mut policy as *mut _ as *mut c_void,
            size: std::mem::size_of::<AccentPolicy>(),
        };
        let _ = call(hwnd, &mut data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN: u16 = VK_LWIN.0;
    const E: u16 = 0x45;
    const LEFT_ARROW: u16 = VK_LEFT.0;
    const SHIFT: u16 = VK_LSHIFT.0;

    fn tap(m: &mut WinKeyMachine, vk: u16, down: bool) -> KeyDecision {
        m.on_key(vk, down, false, false, true, true, 0)
    }

    fn with_shift(m: &mut WinKeyMachine, vk: u16, down: bool) -> KeyDecision {
        m.on_key(vk, down, true, false, true, true, 0)
    }

    #[test]
    fn windows_never_hears_a_lone_start_key() {
        let mut m = WinKeyMachine::default();
        assert!(tap(&mut m, WIN, true).swallow, "the press is held back");
        let up = tap(&mut m, WIN, false);
        assert!(up.swallow, "and so is the release");
        assert_eq!(up.open, Some(Opened::Menu));
        assert!(!up.replay_press && !up.open_native);
    }

    #[test]
    fn every_press_is_decided_the_same_way() {
        let mut m = WinKeyMachine::default();
        for _ in 0..3 {
            assert!(tap(&mut m, WIN, true).swallow);
            let up = tap(&mut m, WIN, false);
            assert!(up.swallow);
            assert_eq!(up.open, Some(Opened::Menu));
        }
    }

    #[test]
    fn holding_the_key_still_tells_windows_nothing() {
        let mut m = WinKeyMachine::default();
        for time in [0, 300, 330, 360] {
            assert!(m.on_key(WIN, true, false, false, true, true, time).swallow);
        }
        assert_eq!(m.on_key(WIN, false, false, false, true, true, 400).open, Some(Opened::Menu));
    }

    #[test]
    fn shortcuts_reach_windows_in_the_right_order() {
        let mut m = WinKeyMachine::default();
        tap(&mut m, WIN, true);
        let e = tap(&mut m, E, true);
        assert_eq!(e.shortcut_key, Some(E), "Windows is told about Start, then gets the key");
        assert!(e.swallow);
        assert_eq!(tap(&mut m, E, false), KeyDecision::default(), "the release goes straight through");
        assert_eq!(tap(&mut m, WIN, false), KeyDecision::default(), "and so does Start's");
    }

    #[test]
    fn a_second_key_in_a_shortcut_goes_straight_through() {
        let mut m = WinKeyMachine::default();
        tap(&mut m, WIN, true);
        assert_eq!(tap(&mut m, LEFT_ARROW, true).shortcut_key, Some(LEFT_ARROW));
        tap(&mut m, LEFT_ARROW, false);
        assert_eq!(tap(&mut m, LEFT_ARROW, true), KeyDecision::default());
        tap(&mut m, LEFT_ARROW, false);
        assert_eq!(tap(&mut m, WIN, false), KeyDecision::default());
    }

    #[test]
    fn start_plus_s_opens_our_menu_with_nothing_left_over() {
        let mut m = WinKeyMachine::default();
        tap(&mut m, WIN, true);
        let s = tap(&mut m, VK_S, true);
        assert!(s.swallow);
        assert_eq!(s.open, Some(Opened::Search));
        assert!(tap(&mut m, VK_S, false).swallow);
        let up = tap(&mut m, WIN, false);
        assert!(up.swallow);
        assert_eq!(up.open, None);
    }

    #[test]
    fn start_plus_l_locks_the_pc() {
        let mut m = WinKeyMachine::default();
        tap(&mut m, WIN, true);
        let l = tap(&mut m, VK_L, true);
        assert!(l.lock && l.swallow);
        assert_eq!(l.shortcut_key, None);
    }

    #[test]
    fn screenshot_shortcut_is_left_alone() {
        let mut m = WinKeyMachine::default();
        tap(&mut m, WIN, true);
        assert_eq!(with_shift(&mut m, SHIFT, true), KeyDecision::default(), "Shift itself isn't a shortcut yet");
        let s = with_shift(&mut m, VK_S, true);
        assert_eq!(s.shortcut_key, Some(VK_S), "Start + Shift + S must still take a screenshot");
        assert_eq!(s.open, None);
        assert_eq!(with_shift(&mut m, WIN, false), KeyDecision::default());
    }

    #[test]
    fn shift_then_start_gives_the_real_menu_on_release() {
        let mut m = WinKeyMachine::default();
        assert!(with_shift(&mut m, WIN, true).swallow);
        let up = with_shift(&mut m, WIN, false);
        assert!(up.swallow && up.open_native);
        assert_eq!(up.open, None);
    }

    #[test]
    fn letting_go_of_shift_first_still_counts() {
        let mut m = WinKeyMachine::default();
        with_shift(&mut m, WIN, true);
        assert!(tap(&mut m, WIN, false).open_native);
    }

    #[test]
    fn ctrl_or_alt_with_start_is_windows_business() {
        let mut m = WinKeyMachine::default();
        assert_eq!(m.on_key(WIN, true, false, true, true, true, 0), KeyDecision::default());
        assert_eq!(m.on_key(LEFT_ARROW, true, false, true, true, true, 0), KeyDecision::default());
        assert_eq!(m.on_key(WIN, false, false, true, true, true, 0), KeyDecision::default());
    }

    #[test]
    fn switched_off_means_nothing_changes_for_the_start_key() {
        let mut m = WinKeyMachine::default();
        m.on_key(WIN, true, false, false, false, true, 0);
        let up = m.on_key(WIN, false, false, false, false, true, 0);
        assert!(up.replay_press, "Windows gets the whole press after all");
        assert_eq!(up.open, None);
    }

    #[test]
    fn a_missed_release_does_not_turn_typing_into_shortcuts() {
        let mut m = WinKeyMachine::default();
        m.on_key(WIN, true, false, false, true, true, 1000);
        // Let go on the lock screen, so the release never arrives. Then, much later:
        assert_eq!(m.on_key(E, true, false, false, true, true, 90_000), KeyDecision::default());
        assert_eq!(m.on_key(WIN, true, false, false, true, true, 91_000).swallow, true);
        assert_eq!(m.on_key(WIN, false, false, false, true, true, 91_050).open, Some(Opened::Menu));
    }

    #[test]
    fn a_mouse_used_with_start_is_a_shortcut() {
        let mut m = WinKeyMachine::default();
        assert!(!m.on_mouse(), "no Start key, nothing to do");
        tap(&mut m, WIN, true);
        assert!(m.on_mouse());
        assert!(!m.on_mouse(), "only tell Windows once");
        assert_eq!(tap(&mut m, WIN, false), KeyDecision::default());
    }
}
