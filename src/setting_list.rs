//! Every Windows setting search can change, and the words that find it.

use crate::i18n::tx;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum QuickId {
    Wifi,
    Bluetooth,
    DarkMode,
    AppsDark,
    SystemDark,
    KeepAwake,
    Mute,
    MicMute,
    Transparency,
    AccentTaskbar,
    AccentTitleBars,
    AutoHideTaskbar,
    TaskbarAlign,
    TaskViewButton,
    ClockSeconds,
    FileExtensions,
    HiddenFiles,
    ItemCheckboxes,
    CompactExplorer,
    ClipboardHistory,
    GameMode,
    Animations,
    SnapWindows,
    DragContents,
    FontSmoothing,
    WindowShadows,
    HoverActivate,
    ScreenSaver,
    PointerPrecision,
    SwapMouseButtons,
    StickyKeys,
    Autostart,
    Volume,
    MicVolume,
    Brightness,
    MouseSpeed,
    ScrollLines,
    DoubleClick,
    KeyRepeatSpeed,
    KeyRepeatDelay,
    ScreenTimeout,
    SleepTimeout,
    PowerMode,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Control {
    Toggle,
    Slider { min: f32, max: f32, step: f32, unit: &'static str },
    Choice(&'static [&'static str]),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Value {
    Loading,
    Unavailable,
    On(bool),
    Number(f32),
    Pick(usize),
}

pub struct Info {
    pub label: &'static str,
    pub glyph: char,
    pub control: Control,
    /// Words that should find this setting in search.
    pub keywords: &'static str,
    /// Where to change it if it can't be changed here.
    pub page: &'static str,
    /// Other names for it, comma separated. Typing one exactly means this setting.
    pub names: &'static str,
    /// Comma separated phrases that already say to turn it on: "mute".
    pub on_phrases: &'static str,
    /// The same for turning it off: "unmute", "light mode".
    pub off_phrases: &'static str,
    /// Words meaning "more of this" and "less of this": "louder", "dimmer".
    pub up_words: &'static str,
    pub down_words: &'static str,
    /// Windows may only show the change after signing out or reopening Explorer.
    pub late: bool,
}

const PERCENT: Control = Control::Slider { min: 0.0, max: 100.0, step: 1.0, unit: "%" };
const MINUTES: Control = Control::Slider { min: 0.0, max: 120.0, step: 1.0, unit: " min" };
const LINES: Control = Control::Slider { min: 1.0, max: 20.0, step: 1.0, unit: "" };
const ALIGNMENT: &[&str] = &[tx("Left"), tx("Center")];
const REPEAT_DELAY: &[&str] = &[tx("Short"), tx("Medium"), tx("Long"), tx("Longest")];
const POWER_MODES: &[&str] = &[tx("Saver"), tx("Balanced"), tx("Best")];

const COLORS: &str = "ms-settings:colors";
const TASKBAR: &str = "ms-settings:taskbar";
const FOR_DEVS: &str = "ms-settings:developers";
const EFFECTS: &str = "ms-settings:easeofaccess-visualeffects";
const MOUSE: &str = "ms-settings:mousetouchpad";
const KEYBOARD: &str = "ms-settings:easeofaccess-keyboard";
const POWER: &str = "ms-settings:powersleep";
const SOUND: &str = "ms-settings:sound";

const fn base(label: &'static str, glyph: char, control: Control, keywords: &'static str, page: &'static str) -> Info {
    Info { label, glyph, control, keywords, page, names: "", on_phrases: "", off_phrases: "", up_words: "", down_words: "", late: false }
}

const fn toggle(label: &'static str, glyph: char, keywords: &'static str, page: &'static str) -> Info {
    base(label, glyph, Control::Toggle, keywords, page)
}

impl QuickId {
    pub const ALL: [QuickId; 43] = [
        QuickId::Wifi,
        QuickId::Bluetooth,
        QuickId::DarkMode,
        QuickId::AppsDark,
        QuickId::SystemDark,
        QuickId::KeepAwake,
        QuickId::Mute,
        QuickId::MicMute,
        QuickId::Transparency,
        QuickId::AccentTaskbar,
        QuickId::AccentTitleBars,
        QuickId::AutoHideTaskbar,
        QuickId::TaskbarAlign,
        QuickId::TaskViewButton,
        QuickId::ClockSeconds,
        QuickId::FileExtensions,
        QuickId::HiddenFiles,
        QuickId::ItemCheckboxes,
        QuickId::CompactExplorer,
        QuickId::ClipboardHistory,
        QuickId::GameMode,
        QuickId::Animations,
        QuickId::SnapWindows,
        QuickId::DragContents,
        QuickId::FontSmoothing,
        QuickId::WindowShadows,
        QuickId::HoverActivate,
        QuickId::ScreenSaver,
        QuickId::PointerPrecision,
        QuickId::SwapMouseButtons,
        QuickId::StickyKeys,
        QuickId::Autostart,
        QuickId::Volume,
        QuickId::MicVolume,
        QuickId::Brightness,
        QuickId::MouseSpeed,
        QuickId::ScrollLines,
        QuickId::DoubleClick,
        QuickId::KeyRepeatSpeed,
        QuickId::KeyRepeatDelay,
        QuickId::ScreenTimeout,
        QuickId::SleepTimeout,
        QuickId::PowerMode,
    ];

    pub fn info(self) -> Info {
        match self {
            QuickId::Wifi => Info { names: "wifi, wlan, wireless", ..toggle(tx("Wi-Fi"), '\u{E701}', "internet network netzwerk", "ms-settings:network-wifi") }, // i18n-ignore: search words
            QuickId::Bluetooth => Info { names: "bt", ..toggle(tx("Bluetooth"), '\u{E702}', "headphones wireless devices", "ms-settings:bluetooth") }, // i18n-ignore: search words
            QuickId::DarkMode => Info {
                names: "dark, dark theme, dunkelmodus", // i18n-ignore: search words
                on_phrases: "go dark", // i18n-ignore: search words
                off_phrases: "light mode, light theme, heller modus, hellmodus, go light", // i18n-ignore: search words
                ..toggle(tx("Dark mode"), '\u{E708}', "theme night black colors appearance dunkel farben", COLORS) // i18n-ignore: search words
            },
            QuickId::AppsDark => Info { off_phrases: "light mode for apps, light apps", ..toggle(tx("Dark mode for apps"), '\u{E708}', "theme applications programs light", COLORS) }, // i18n-ignore: search words
            QuickId::SystemDark => Info {
                names: "dark mode for windows, dark windows", // i18n-ignore: search words
                off_phrases: "light taskbar, light mode for windows", // i18n-ignore: search words
                ..toggle(tx("Dark taskbar and Start"), '\u{E708}', "theme windows system light menu", COLORS) // i18n-ignore: search words
            },
            QuickId::KeepAwake => Info {
                on_phrases: "keep awake, stay awake, keep screen on, caffeine", // i18n-ignore: search words
                off_phrases: "let it sleep", // i18n-ignore: search words
                ..toggle(tx("Keep awake"), '\u{E7F4}', "sleep prevent screen timeout wach", POWER) // i18n-ignore: search words
            },
            QuickId::Mute => Info {
                on_phrases: "mute, stumm, silence, sound off, off sound, ton aus", // i18n-ignore: search words
                off_phrases: "unmute, sound on, on sound, ton an", // i18n-ignore: search words
                ..toggle(tx("Mute"), '\u{E74F}', "sound audio speaker stummschalten ton", SOUND) // i18n-ignore: search words
            },
            QuickId::MicMute => Info {
                on_phrases: "mute mic, mute microphone, mic off, off mic, microphone off, off microphone, mikrofon aus", // i18n-ignore: search words
                off_phrases: "unmute mic, unmute microphone, mic on, on mic, microphone on, on microphone, mikrofon an", // i18n-ignore: search words
                ..toggle(tx("Mic mute"), '\u{E720}', "microphone mikrofon silence recording input", SOUND) // i18n-ignore: search words
            },
            QuickId::Transparency => Info { names: "transparency, transparency effects, transparenz", ..toggle(tx("See-through effects"), '\u{E790}', "acrylic blur glass", COLORS) }, // i18n-ignore: search words
            QuickId::AccentTaskbar => toggle(tx("Accent color on taskbar"), '\u{E790}', "colour start menu personalize akzentfarbe", COLORS), // i18n-ignore: search words
            QuickId::AccentTitleBars => toggle(tx("Accent color on title bars"), '\u{E790}', "colour window windows borders akzentfarbe", COLORS), // i18n-ignore: search words
            QuickId::AutoHideTaskbar => Info {
                on_phrases: "hide taskbar, auto hide taskbar, autohide taskbar", // i18n-ignore: search words
                off_phrases: "show taskbar, always show taskbar", // i18n-ignore: search words
                ..toggle(tx("Hide taskbar"), '\u{E7C4}', "auto autohide taskleiste", TASKBAR) // i18n-ignore: search words
            },
            QuickId::TaskbarAlign => Info {
                names: "align taskbar", // i18n-ignore: search words
                late: true,
                ..base(tx("Taskbar alignment"), '\u{E7C4}', Control::Choice(ALIGNMENT), "taskbar start menu position taskleiste", TASKBAR) // i18n-ignore: search words
            },
            QuickId::TaskViewButton => Info { on_phrases: "show task view", off_phrases: "hide task view", late: true, ..toggle(tx("Task view button"), '\u{E7C4}', "taskbar desktops", TASKBAR) }, // i18n-ignore: search words
            QuickId::ClockSeconds => Info { on_phrases: "show seconds", off_phrases: "hide seconds", late: true, ..toggle(tx("Seconds in the clock"), '\u{E823}', "taskbar time uhr sekunden", TASKBAR) }, // i18n-ignore: search words
            QuickId::FileExtensions => Info {
                on_phrases: "show file extensions, show extensions", // i18n-ignore: search words
                off_phrases: "hide file extensions, hide extensions", // i18n-ignore: search words
                ..toggle(tx("File extensions"), '\u{E7C3}', "explorer types dateiendungen", FOR_DEVS) // i18n-ignore: search words
            },
            QuickId::HiddenFiles => Info { on_phrases: "show hidden", off_phrases: "hide hidden", ..toggle(tx("Hidden files"), '\u{E890}', "explorer folders versteckte", FOR_DEVS) }, // i18n-ignore: search words
            QuickId::ItemCheckboxes => Info { late: true, ..toggle(tx("File checkboxes"), '\u{E73A}', "explorer select item check boxes", FOR_DEVS) }, // i18n-ignore: search words
            QuickId::CompactExplorer => Info { late: true, ..toggle(tx("Compact file view"), '\u{E8FD}', "explorer dense spacing", FOR_DEVS) }, // i18n-ignore: search words
            QuickId::ClipboardHistory => Info { names: "zwischenablage", late: true, ..toggle(tx("Clipboard history"), '\u{E77F}', "copy paste", "ms-settings:clipboard") }, // i18n-ignore: search words
            QuickId::GameMode => Info { names: "spielmodus", late: true, ..toggle(tx("Game Mode"), '\u{E7FC}', "gaming games performance", "ms-settings:gaming-gamemode") }, // i18n-ignore: search words
            QuickId::Animations => Info { off_phrases: "reduce motion", ..toggle(tx("Animations"), '\u{E945}', "effects motion transitions animationen", EFFECTS) }, // i18n-ignore: search words
            QuickId::SnapWindows => toggle(tx("Snap windows"), '\u{E7C4}', "arrange aero multitasking fenster", "ms-settings:multitasking"), // i18n-ignore: search words
            QuickId::DragContents => toggle(tx("Show window contents while dragging"), '\u{E7C4}', "move performance", EFFECTS), // i18n-ignore: search words
            QuickId::FontSmoothing => Info { names: "cleartype, font smoothing", ..toggle(tx("Smooth fonts"), '\u{E8D2}', "text blurry schrift", "ms-settings:display") }, // i18n-ignore: search words
            QuickId::WindowShadows => Info { names: "drop shadow", ..toggle(tx("Window shadows"), '\u{E7C4}', "effects schatten", EFFECTS) }, // i18n-ignore: search words
            QuickId::HoverActivate => Info { names: "focus follows mouse", ..toggle(tx("Focus window on hover"), '\u{E962}', "activate tracking pointer", MOUSE) }, // i18n-ignore: search words
            QuickId::ScreenSaver => Info { names: "screensaver, bildschirmschoner", ..toggle(tx("Screen saver"), '\u{E7F4}', "idle", "ms-settings:lockscreen") }, // i18n-ignore: search words
            QuickId::PointerPrecision => Info { names: "mouse acceleration, mausbeschleunigung", ..toggle(tx("Enhance pointer precision"), '\u{E962}', "mouse cursor", MOUSE) }, // i18n-ignore: search words
            QuickId::SwapMouseButtons => toggle(tx("Swap mouse buttons"), '\u{E962}', "primary button maustasten", MOUSE), // i18n-ignore: search words
            QuickId::StickyKeys => Info { names: "einrastfunktion", ..toggle(tx("Sticky Keys"), '\u{E765}', "accessibility keyboard shift", KEYBOARD) }, // i18n-ignore: search words
            QuickId::Autostart => Info { names: "autostart", ..toggle(tx("Start Startline with Windows"), '\u{E7E8}', "login boot launch startup", "ms-settings:startupapps") }, // i18n-ignore: search words
            QuickId::Volume => Info {
                names: "sound level, lautstärke", // i18n-ignore: search words
                up_words: "louder lauter", // i18n-ignore: search words
                down_words: "quieter softer leiser", // i18n-ignore: search words
                ..base(tx("Volume"), '\u{E767}', PERCENT, "sound audio speaker loud ton", SOUND) // i18n-ignore: search words
            },
            QuickId::MicVolume => Info { names: "mic volume, microphone volume", ..base(tx("Mic level"), '\u{E720}', PERCENT, "microphone mikrofon input recording gain", SOUND) }, // i18n-ignore: search words
            QuickId::Brightness => Info {
                names: "helligkeit", // i18n-ignore: search words
                up_words: "brighter heller", // i18n-ignore: search words
                down_words: "dimmer dim darker dunkler", // i18n-ignore: search words
                ..base(tx("Brightness"), '\u{E706}', PERCENT, "display screen monitor bildschirm", "ms-settings:display") // i18n-ignore: search words
            },
            QuickId::MouseSpeed => Info { up_words: "faster", down_words: "slower", ..base(tx("Mouse speed"), '\u{E962}', LINES, "pointer cursor sensitivity maus geschwindigkeit", MOUSE) }, // i18n-ignore: search words
            QuickId::ScrollLines => Info { names: "scroll speed", ..base(tx("Scroll lines"), '\u{E962}', LINES, "mouse wheel mausrad", MOUSE) }, // i18n-ignore: search words
            QuickId::DoubleClick => Info { names: "double click speed", ..base(tx("Double-click time"), '\u{E962}', Control::Slider { min: 200.0, max: 900.0, step: 50.0, unit: " ms" }, "mouse doppelklick doubleclick", MOUSE) }, // i18n-ignore: search words
            QuickId::KeyRepeatSpeed => base(tx("Key repeat speed"), '\u{E765}', Control::Slider { min: 0.0, max: 31.0, step: 1.0, unit: "" }, "keyboard typing tastatur rate", KEYBOARD), // i18n-ignore: search words
            QuickId::KeyRepeatDelay => base(tx("Key repeat delay"), '\u{E765}', Control::Choice(REPEAT_DELAY), "keyboard typing tastatur", KEYBOARD), // i18n-ignore: search words
            QuickId::ScreenTimeout => Info { names: "screen timeout, display timeout", ..base(tx("Screen off after"), '\u{E7F4}', MINUTES, "monitor display idle bildschirm", POWER) }, // i18n-ignore: search words
            QuickId::SleepTimeout => Info { names: "sleep timeout", ..base(tx("Sleep after"), '\u{E708}', MINUTES, "standby idle energy", POWER) }, // i18n-ignore: search words
            QuickId::PowerMode => base(tx("Power mode"), '\u{E945}', Control::Choice(POWER_MODES), "performance battery efficiency energy saver balanced best energiesparen", POWER), // i18n-ignore: search words
        }
    }
}
