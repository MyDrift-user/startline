#![windows_subsystem = "windows"]

mod actions;
mod apps;
mod bookmarks_page;
mod bookmarks;
mod browser;
mod calc;
mod classic;
mod config;
mod customize;
mod home;
mod i18n;
mod icons;
mod index;
mod instance;
mod intent;
mod m365;
mod matching;
mod now_playing;
mod media;
mod pins_page;
mod platform;
mod policy;
mod popup;
mod providers;
mod quick;
mod scope;
mod search;
mod search_page;
mod setting_list;
mod settings;
mod sqlite;
mod styles;
mod system;
mod ui;
mod update;
mod widgets;

use std::path::PathBuf;
use std::time::Duration;

/// What the command line asked for.
#[derive(Default, Clone)]
pub struct Options {
    /// Save a picture of the window here and exit.
    pub screenshot: Option<PathBuf>,
    /// Start out of sight, as when Windows starts it at login.
    pub background: bool,
    /// Skip the Start key connection, keeping only the tray icon.
    pub no_hook: bool,
    /// Treat simulated key presses as real ones, for automated tests of the Start key.
    pub test_injected_keys: bool,
    pub query: Option<String>,
    pub page: Option<String>,
    pub preset: Option<String>,
    pub experience: Option<String>,
    /// For pictures of Settings: open the folder tree and a pin, and make the window tall.
    pub demo_open: bool,
    /// For a picture of the search box's right-click menu: its text selected and the menu open.
    pub demo_text_menu: bool,
    /// For a picture of a lower part of a Settings page: scrolled down this far.
    pub demo_scroll: Option<f32>,
    /// For a picture of the Microsoft 365 parts: a made-up signed-in account and example events, nothing sent anywhere.
    pub demo_m365: bool,
    /// For a picture of the Now playing card: a made-up song, nothing asked of Windows.
    pub demo_media: bool,
    /// For a picture of what policies do: read them from this file instead of the Registry.
    pub test_policies: Option<PathBuf>,
    /// Started by the copy an update replaces, which may still be ending.
    pub replacing: bool,
}

fn parse_options() -> (Options, Option<PathBuf>, Option<PathBuf>) {
    let mut options = Options::default();
    let mut probe = None;
    let mut probe_changes = None;
    let mut args = std::env::args().skip(1).peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--screenshot" => options.screenshot = args.next().map(PathBuf::from),
            "--background" => options.background = true,
            "--no-hook" => options.no_hook = true,
            "--test-injected-keys" => options.test_injected_keys = true,
            "--query" => {
                // All the words up to the next option, however the caller quoted them.
                let mut words = Vec::new();
                while let Some(word) = args.next_if(|a| !a.starts_with("--")) {
                    words.push(word);
                }
                options.query = Some(words.join(" "));
            }
            "--page" => options.page = args.next(),
            "--preset" => options.preset = args.next(),
            "--experience" => options.experience = args.next(),
            "--demo-open" => options.demo_open = true,
            "--demo-text-menu" => options.demo_text_menu = true,
            "--demo-m365" => options.demo_m365 = true,
            "--demo-media" => options.demo_media = true,
            "--demo-scroll" => options.demo_scroll = args.next().and_then(|p| p.parse().ok()),
            "--test-policies" => options.test_policies = args.next().map(PathBuf::from),
            // For installer tests and admins: which Microsoft 365 app registration and tenant this copy uses, and why.
            "--m365-source" => {
                let mut cfg = config::Config::load();
                policy::Policies::read().enforce(&mut cfg);
                let say = |what: &str, found: Option<(String, m365::Source)>| match found {
                    Some((value, from)) => println!("{what} {from:?} {value}"),
                    None => println!("{what} None"),
                };
                say("client", m365::client_source(&cfg));
                say("tenant", m365::tenant_source(&cfg));
                std::process::exit(0);
            }
            "--write-policy-templates" => {
                let folder = args.next().map_or_else(|| PathBuf::from("PolicyDefinitions"), PathBuf::from);
                let done = policy::write_templates(&folder);
                std::process::exit(i32::from(done.is_err()));
            }
            // Started by the copy it replaces, which is still ending.
            "--wait-for" => {
                options.replacing = true;
                if let Some(pid) = args.next().and_then(|p| p.parse().ok()) {
                    update::wait_for(pid);
                }
            }
            "--probe" => probe = args.next().map(PathBuf::from),
            "--probe-changes" => probe_changes = args.next().map(PathBuf::from),
            _ => {}
        }
    }
    (options, probe, probe_changes)
}

/// The app's own picture: a rounded tile holding a grid of squares with one bright dot.
pub fn app_icon_pixels(size: usize) -> Vec<u8> {
    let mut rgba = vec![0u8; size * size * 4];
    let s = size as f32;
    let radius = s * 0.22;
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let (dx, dy) = ((fx - s / 2.0).abs() - (s / 2.0 - radius), (fy - s / 2.0).abs() - (s / 2.0 - radius));
            let outside = dx.max(0.0).hypot(dy.max(0.0)) - radius;
            let alpha = (0.5 - outside).clamp(0.0, 1.0);
            let mut color = [39u8, 39, 37];
            // Four squares; the last one is a circle, the search dot.
            let (u, v) = (fx / s, fy / s);
            let in_square = |lo: f32, hi: f32, p: f32| p >= lo && p <= hi;
            let left = in_square(0.24, 0.46, u);
            let right = in_square(0.54, 0.76, u);
            let top = in_square(0.24, 0.46, v);
            let bottom = in_square(0.54, 0.76, v);
            let dot = (u - 0.65).hypot(v - 0.65) <= 0.12;
            if (left && (top || bottom)) || (right && top) {
                color = [236, 234, 229];
            }
            if dot {
                color = [217, 164, 107];
            }
            let at = (y * size + x) * 4;
            rgba[at..at + 3].copy_from_slice(&color);
            rgba[at + 3] = (alpha * 255.0) as u8;
        }
    }
    rgba
}

fn main() {
    // Runs that do one job and end (--version, the update helper, writing the templates...) come first; they never keep
    // a copy running. Everything else is a copy of the menu, and the first thing it does is make sure it's the only one.
    if let Some(code) = update::handle_command_line() {
        std::process::exit(code);
    }
    let (options, probe, probe_changes) = parse_options();
    // Pictures and the automated key tests run beside a copy that may be running, and leave the keyboard to it.
    let _one_copy = if options.screenshot.is_none() && !options.test_injected_keys && probe.is_none() && probe_changes.is_none() {
        // The copy an update replaces is still ending: wait for it rather than give way to it. Copies started in the
        // background (at logon, or again after an update's installer closed the old one) wait a little too, in case
        // the one before is just ending.
        let wait = if options.replacing {
            Duration::from_secs(20)
        } else if options.background {
            Duration::from_secs(5)
        } else {
            Duration::ZERO
        };
        match instance::claim(&instance::session_name(), wait) {
            Some(claim) => Some(claim),
            // Nobody asked for a menu: a second autostart entry mustn't open it at logon.
            None if options.background => return,
            None => {
                // Another copy runs: it opens its menu, as a second launch always did. It may still be starting, so
                // its window is looked for a few seconds, and never longer.
                system::signal_running_instance(Duration::from_secs(10));
                return;
            }
        }
    } else {
        None
    };

    if let Some(path) = probe {
        // The same screen coordinates the running menu uses.
        let _ = unsafe { windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        let mut report = settings::probe_report();
        report.push_str(&system::probe_start_button());
        report.push_str(&quick::probe_report());
        report.push_str(&bookmarks::probe_report());
        report.push_str(&media::probe_report());
        let _ = std::fs::write(path, report);
        return;
    }
    if let Some(path) = probe_changes {
        let _ = std::fs::write(path, settings::probe_changes_report());
        return;
    }

    update::clean_after_update();

    let cfg = config::Config::load();
    // So a message before the window opens is in the right language too.
    i18n::set(i18n::Lang::from_setting(&cfg.language));
    let viewport = egui::ViewportBuilder::default()
        .with_title("Startline") // i18n-ignore: the app's name
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_taskbar(false)
        .with_always_on_top()
        .with_inner_size([cfg.width, cfg.height])
        .with_icon(egui::IconData { rgba: app_icon_pixels(64), width: 64, height: 64 });
    let native = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        depth_buffer: 0,
        multisampling: 0,
        persist_window: false,
        ..Default::default()
    };
    let result = eframe::run_native(
        "Startline", // i18n-ignore: the app's name
        native,
        Box::new(move |cc| Ok(Box::new(ui::Menu::new(cc, cfg, options)))),
    );
    if let Err(error) = result {
        platform::message_box_error(&crate::trf!("Startline couldn't open its window.\n\n{error}", error = error));
    }
}
