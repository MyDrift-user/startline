//! Every Windows setting search can change, and the words that find it.

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
            QuickId::Wifi => Info { names: "wifi, wlan, wireless", ..toggle("Wi-Fi", '\u{E701}', "internet network netzwerk", "ms-settings:network-wifi") },
            QuickId::Bluetooth => Info { names: "bt", ..toggle("Bluetooth", '\u{E702}', "headphones wireless devices", "ms-settings:bluetooth") },
            QuickId::DarkMode => Info {
                names: "dark, dark theme, dunkelmodus",
                on_phrases: "go dark",
                off_phrases: "light mode, light theme, heller modus, hellmodus, go light",
                ..toggle("Dark mode", '\u{E708}', "theme night black colors appearance dunkel farben", COLORS)
            },
            QuickId::AppsDark => Info { off_phrases: "light mode for apps, light apps", ..toggle("Dark mode for apps", '\u{E708}', "theme applications programs light", COLORS) },
            QuickId::SystemDark => Info {
                names: "dark mode for windows, dark windows",
                off_phrases: "light taskbar, light mode for windows",
                ..toggle("Dark taskbar and Start", '\u{E708}', "theme windows system light menu", COLORS)
            },
            QuickId::KeepAwake => Info {
                on_phrases: "keep awake, stay awake, keep screen on, caffeine",
                off_phrases: "let it sleep",
                ..toggle("Keep awake", '\u{E7F4}', "sleep prevent screen timeout wach", POWER)
            },
            QuickId::Mute => Info {
                on_phrases: "mute, stumm, silence, sound off, off sound, ton aus",
                off_phrases: "unmute, sound on, on sound, ton an",
                ..toggle("Mute", '\u{E74F}', "sound audio speaker stummschalten ton", SOUND)
            },
            QuickId::MicMute => Info {
                on_phrases: "mute mic, mute microphone, mic off, off mic, microphone off, off microphone, mikrofon aus",
                off_phrases: "unmute mic, unmute microphone, mic on, on mic, microphone on, on microphone, mikrofon an",
                ..toggle("Mic mute", '\u{E720}', "microphone mikrofon silence recording input", SOUND)
            },
            QuickId::Transparency => Info { names: "transparency, transparency effects, transparenz", ..toggle("See-through effects", '\u{E790}', "acrylic blur glass", COLORS) },
            QuickId::AccentTaskbar => toggle("Accent color on taskbar", '\u{E790}', "colour start menu personalize akzentfarbe", COLORS),
            QuickId::AccentTitleBars => toggle("Accent color on title bars", '\u{E790}', "colour window windows borders akzentfarbe", COLORS),
            QuickId::AutoHideTaskbar => Info {
                on_phrases: "hide taskbar, auto hide taskbar, autohide taskbar",
                off_phrases: "show taskbar, always show taskbar",
                ..toggle("Hide taskbar", '\u{E7C4}', "auto autohide taskleiste", TASKBAR)
            },
            QuickId::TaskbarAlign => Info {
                names: "align taskbar",
                late: true,
                ..base("Taskbar alignment", '\u{E7C4}', Control::Choice(&["Left", "Center"]), "taskbar start menu position taskleiste", TASKBAR)
            },
            QuickId::TaskViewButton => Info { on_phrases: "show task view", off_phrases: "hide task view", late: true, ..toggle("Task view button", '\u{E7C4}', "taskbar desktops", TASKBAR) },
            QuickId::ClockSeconds => Info { on_phrases: "show seconds", off_phrases: "hide seconds", late: true, ..toggle("Seconds in the clock", '\u{E823}', "taskbar time uhr sekunden", TASKBAR) },
            QuickId::FileExtensions => Info {
                on_phrases: "show file extensions, show extensions",
                off_phrases: "hide file extensions, hide extensions",
                ..toggle("File extensions", '\u{E7C3}', "explorer types dateiendungen", FOR_DEVS)
            },
            QuickId::HiddenFiles => Info { on_phrases: "show hidden", off_phrases: "hide hidden", ..toggle("Hidden files", '\u{E890}', "explorer folders versteckte", FOR_DEVS) },
            QuickId::ItemCheckboxes => Info { late: true, ..toggle("File checkboxes", '\u{E73A}', "explorer select item check boxes", FOR_DEVS) },
            QuickId::CompactExplorer => Info { late: true, ..toggle("Compact file view", '\u{E8FD}', "explorer dense spacing", FOR_DEVS) },
            QuickId::ClipboardHistory => Info { names: "zwischenablage", late: true, ..toggle("Clipboard history", '\u{E77F}', "copy paste", "ms-settings:clipboard") },
            QuickId::GameMode => Info { names: "spielmodus", late: true, ..toggle("Game Mode", '\u{E7FC}', "gaming games performance", "ms-settings:gaming-gamemode") },
            QuickId::Animations => Info { off_phrases: "reduce motion", ..toggle("Animations", '\u{E945}', "effects motion transitions animationen", EFFECTS) },
            QuickId::SnapWindows => toggle("Snap windows", '\u{E7C4}', "arrange aero multitasking fenster", "ms-settings:multitasking"),
            QuickId::DragContents => toggle("Show window contents while dragging", '\u{E7C4}', "move performance", EFFECTS),
            QuickId::FontSmoothing => Info { names: "cleartype, font smoothing", ..toggle("Smooth fonts", '\u{E8D2}', "text blurry schrift", "ms-settings:display") },
            QuickId::WindowShadows => Info { names: "drop shadow", ..toggle("Window shadows", '\u{E7C4}', "effects schatten", EFFECTS) },
            QuickId::HoverActivate => Info { names: "focus follows mouse", ..toggle("Focus window on hover", '\u{E962}', "activate tracking pointer", MOUSE) },
            QuickId::ScreenSaver => Info { names: "screensaver, bildschirmschoner", ..toggle("Screen saver", '\u{E7F4}', "idle", "ms-settings:lockscreen") },
            QuickId::PointerPrecision => Info { names: "mouse acceleration, mausbeschleunigung", ..toggle("Enhance pointer precision", '\u{E962}', "mouse cursor", MOUSE) },
            QuickId::SwapMouseButtons => toggle("Swap mouse buttons", '\u{E962}', "primary button maustasten", MOUSE),
            QuickId::StickyKeys => Info { names: "einrastfunktion", ..toggle("Sticky Keys", '\u{E765}', "accessibility keyboard shift", KEYBOARD) },
            QuickId::Autostart => Info { names: "autostart", ..toggle("Start Startline with Windows", '\u{E7E8}', "login boot launch startup", "ms-settings:startupapps") },
            QuickId::Volume => Info {
                names: "sound level, lautstärke",
                up_words: "louder lauter",
                down_words: "quieter softer leiser",
                ..base("Volume", '\u{E767}', PERCENT, "sound audio speaker loud ton", SOUND)
            },
            QuickId::MicVolume => Info { names: "mic volume, microphone volume", ..base("Mic level", '\u{E720}', PERCENT, "microphone mikrofon input recording gain", SOUND) },
            QuickId::Brightness => Info {
                names: "helligkeit",
                up_words: "brighter heller",
                down_words: "dimmer dim darker dunkler",
                ..base("Brightness", '\u{E706}', PERCENT, "display screen monitor bildschirm", "ms-settings:display")
            },
            QuickId::MouseSpeed => Info { up_words: "faster", down_words: "slower", ..base("Mouse speed", '\u{E962}', LINES, "pointer cursor sensitivity maus geschwindigkeit", MOUSE) },
            QuickId::ScrollLines => Info { names: "scroll speed", ..base("Scroll lines", '\u{E962}', LINES, "mouse wheel mausrad", MOUSE) },
            QuickId::DoubleClick => Info { names: "double click speed", ..base("Double-click time", '\u{E962}', Control::Slider { min: 200.0, max: 900.0, step: 50.0, unit: " ms" }, "mouse doppelklick doubleclick", MOUSE) },
            QuickId::KeyRepeatSpeed => base("Key repeat speed", '\u{E765}', Control::Slider { min: 0.0, max: 31.0, step: 1.0, unit: "" }, "keyboard typing tastatur rate", KEYBOARD),
            QuickId::KeyRepeatDelay => base("Key repeat delay", '\u{E765}', Control::Choice(&["Short", "Medium", "Long", "Longest"]), "keyboard typing tastatur", KEYBOARD),
            QuickId::ScreenTimeout => Info { names: "screen timeout, display timeout", ..base("Screen off after", '\u{E7F4}', MINUTES, "monitor display idle bildschirm", POWER) },
            QuickId::SleepTimeout => Info { names: "sleep timeout", ..base("Sleep after", '\u{E708}', MINUTES, "standby idle energy", POWER) },
            QuickId::PowerMode => base("Power mode", '\u{E945}', Control::Choice(&["Saver", "Balanced", "Best"]), "performance battery efficiency energy saver balanced best energiesparen", POWER),
        }
    }
}
