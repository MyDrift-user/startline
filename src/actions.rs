//! Things search can do besides opening apps and files: jump to a Windows settings page,
//! lock the screen, restart, and so on.

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Do {
    Open(&'static str),
    Lock,
    Sleep,
    SignOut,
    SwitchUser,
    Restart,
    ShutDown,
    EmptyRecycleBin,
    Customize,
    PinPage,
    RealStart,
    Quit,
}

impl Do {
    /// Actions that end what the person is doing ask for a second click.
    pub fn needs_confirm(self) -> bool {
        matches!(self, Do::SignOut | Do::Restart | Do::ShutDown | Do::EmptyRecycleBin)
    }
}

pub struct Action {
    pub name: &'static str,
    pub keywords: &'static str,
    pub glyph: char,
    pub what: Do,
}

const fn page(name: &'static str, keywords: &'static str, glyph: char, target: &'static str) -> Action {
    Action { name, keywords, glyph, what: Do::Open(target) }
}

const fn act(name: &'static str, keywords: &'static str, glyph: char, what: Do) -> Action {
    Action { name, keywords, glyph, what }
}

pub const ACTIONS: &[Action] = &[
    act(crate::i18n::tx("Lock screen"), "lock computer away secure", '\u{E72E}', Do::Lock), // i18n-ignore: search words
    act(crate::i18n::tx("Sleep"), "suspend standby", '\u{E708}', Do::Sleep), // i18n-ignore: search words
    act(crate::i18n::tx("Sign out"), "log off logout user", '\u{E77B}', Do::SignOut), // i18n-ignore: search words
    act(crate::i18n::tx("Switch user"), "change account other person", '\u{E748}', Do::SwitchUser), // i18n-ignore: search words
    act(crate::i18n::tx("Restart"), "reboot computer pc", '\u{E777}', Do::Restart), // i18n-ignore: search words
    act(crate::i18n::tx("Shut down"), "power off turn off shutdown computer pc", '\u{E7E8}', Do::ShutDown), // i18n-ignore: search words
    act(crate::i18n::tx("Empty Recycle Bin"), "trash delete clean", '\u{E74D}', Do::EmptyRecycleBin), // i18n-ignore: search words
    act(crate::i18n::tx("Customize Startline"), "settings preferences theme layout start menu options", '\u{E771}', Do::Customize), // i18n-ignore: search words
    act(crate::i18n::tx("Pin something to Start"), "add app shortcut website file folder favorite", '\u{E718}', Do::PinPage), // i18n-ignore: search words
    act(crate::i18n::tx("Open the Windows Start menu"), "real original native", '\u{E8FC}', Do::RealStart), // i18n-ignore: search words
    act(crate::i18n::tx("Quit Startline"), "exit close", '\u{E711}', Do::Quit), // i18n-ignore: search words
    page(crate::i18n::tx("Display settings"), "screen resolution scale monitor", '\u{E7F4}', "ms-settings:display"), // i18n-ignore: search words
    page(crate::i18n::tx("Night light settings"), "blue light warm eyes night", '\u{E706}', "ms-settings:nightlight"), // i18n-ignore: search words
    page(crate::i18n::tx("Sound settings"), "audio speakers microphone output input", '\u{E767}', "ms-settings:sound"), // i18n-ignore: search words
    page(crate::i18n::tx("Notifications"), "alerts banners toast", '\u{EA8F}', "ms-settings:notifications"), // i18n-ignore: search words
    page(crate::i18n::tx("Do not disturb and focus"), "focus assist quiet hours", '\u{E708}', "ms-settings:quiethours"), // i18n-ignore: search words
    page(crate::i18n::tx("Power and battery"), "sleep energy saver timeout", '\u{E83F}', "ms-settings:powersleep"), // i18n-ignore: search words
    page(crate::i18n::tx("Storage"), "disk space cleanup drive full", '\u{EDA2}', "ms-settings:storagesense"), // i18n-ignore: search words
    page(crate::i18n::tx("Multitasking"), "snap windows desktops alt tab", '\u{E7C4}', "ms-settings:multitasking"), // i18n-ignore: search words
    page(crate::i18n::tx("Wi-Fi settings"), "wifi wireless networks", '\u{E701}', "ms-settings:network-wifi"), // i18n-ignore: search words
    page(crate::i18n::tx("Bluetooth and devices"), "pair headphones keyboard mouse", '\u{E702}', "ms-settings:bluetooth"), // i18n-ignore: search words
    page(crate::i18n::tx("Network status"), "internet ethernet connection ip", '\u{E774}', "ms-settings:network-status"), // i18n-ignore: search words
    page(crate::i18n::tx("VPN"), "tunnel private network", '\u{E705}', "ms-settings:network-vpn"), // i18n-ignore: search words
    page(crate::i18n::tx("Proxy"), "network", '\u{E774}', "ms-settings:network-proxy"), // i18n-ignore: search words
    page(crate::i18n::tx("Airplane mode"), "flight radios offline", '\u{E709}', "ms-settings:network-airplanemode"), // i18n-ignore: search words
    page(crate::i18n::tx("Wallpaper"), "background desktop picture personalize", '\u{E8B9}', "ms-settings:personalization-background"), // i18n-ignore: search words
    page(crate::i18n::tx("Colors and accent"), "theme accent dark light", '\u{E790}', "ms-settings:colors"), // i18n-ignore: search words
    page(crate::i18n::tx("Themes"), "personalize look", '\u{E771}', "ms-settings:themes"), // i18n-ignore: search words
    page(crate::i18n::tx("Lock screen settings"), "personalize", '\u{E72E}', "ms-settings:lockscreen"), // i18n-ignore: search words
    page(crate::i18n::tx("Taskbar settings"), "icons tray corner", '\u{E7C4}', "ms-settings:taskbar"), // i18n-ignore: search words
    page(crate::i18n::tx("Installed apps"), "uninstall remove programs features", '\u{E71D}', "ms-settings:appsfeatures"), // i18n-ignore: search words
    page(crate::i18n::tx("Default apps"), "open with browser associations", '\u{E8A7}', "ms-settings:defaultapps"), // i18n-ignore: search words
    page(crate::i18n::tx("Startup apps"), "autostart login boot", '\u{E7E8}', "ms-settings:startupapps"), // i18n-ignore: search words
    page(crate::i18n::tx("Your account"), "profile user", '\u{E77B}', "ms-settings:yourinfo"), // i18n-ignore: search words
    page(crate::i18n::tx("Sign-in options"), "password pin hello fingerprint", '\u{E72E}', "ms-settings:signinoptions"), // i18n-ignore: search words
    page(crate::i18n::tx("Date and time"), "clock timezone", '\u{E823}', "ms-settings:dateandtime"), // i18n-ignore: search words
    page(crate::i18n::tx("Language and region"), "keyboard locale translate", '\u{E774}', "ms-settings:regionlanguage"), // i18n-ignore: search words
    page(crate::i18n::tx("Typing and keyboard"), "autocorrect touch keyboard", '\u{E765}', "ms-settings:typing"), // i18n-ignore: search words
    page(crate::i18n::tx("Mouse settings"), "pointer cursor scroll touchpad", '\u{E962}', "ms-settings:mousetouchpad"), // i18n-ignore: search words
    page(crate::i18n::tx("Printers and scanners"), "print", '\u{E749}', "ms-settings:printers"), // i18n-ignore: search words
    page(crate::i18n::tx("Windows Update"), "patch upgrade security", '\u{E777}', "ms-settings:windowsupdate"), // i18n-ignore: search words
    page(crate::i18n::tx("Privacy and security"), "permissions camera location", '\u{E72E}', "ms-settings:privacy"), // i18n-ignore: search words
    page(crate::i18n::tx("Windows Security"), "antivirus defender firewall", '\u{E83D}', "windowsdefender:"), // i18n-ignore: search words
    page(crate::i18n::tx("About this PC"), "system info specs rename name version", '\u{E946}', "ms-settings:about"), // i18n-ignore: search words
    page(crate::i18n::tx("All Windows settings"), "preferences control", '\u{E713}', "ms-settings:"), // i18n-ignore: search words
    page(crate::i18n::tx("Control Panel"), "classic settings", '\u{E713}', "control"), // i18n-ignore: search words
    page(crate::i18n::tx("Task Manager"), "processes kill performance taskmgr", '\u{E9D9}', "taskmgr"), // i18n-ignore: search words
    page(crate::i18n::tx("Device Manager"), "drivers hardware devmgmt", '\u{E7F8}', "devmgmt.msc"), // i18n-ignore: search words
    page(crate::i18n::tx("Disk Management"), "partition format diskmgmt", '\u{EDA2}', "diskmgmt.msc"), // i18n-ignore: search words
    page(crate::i18n::tx("Services"), "background services msc", '\u{E713}', "services.msc"), // i18n-ignore: search words
    page(crate::i18n::tx("Registry Editor"), "regedit", '\u{E943}', "regedit"), // i18n-ignore: search words
    page(crate::i18n::tx("Recycle Bin"), "trash deleted", '\u{E74D}', "shell:RecycleBinFolder"), // i18n-ignore: search words
    page(crate::i18n::tx("Downloads"), "files folder", '\u{E896}', "shell:Downloads"), // i18n-ignore: search words
    page(crate::i18n::tx("Documents"), "files folder", '\u{E8A5}', "shell:Personal"), // i18n-ignore: search words
    page(crate::i18n::tx("Pictures"), "photos images folder", '\u{E8B9}', "shell:My Pictures"), // i18n-ignore: search words
    page(crate::i18n::tx("Startup folder"), "autostart shell", '\u{E8B7}', "shell:Startup"), // i18n-ignore: search words
];

pub fn run_system(what: Do) {
    use windows::Win32::System::Power::SetSuspendState;
    use windows::Win32::System::Shutdown::LockWorkStation;
    use windows::Win32::UI::Shell::SHEmptyRecycleBinW;
    use windows::core::PCWSTR;
    let shutdown = |args: &'static str| {
        crate::platform::open_async("", "shutdown.exe".into(), args.into());
    };
    match what {
        Do::Open(target) => crate::platform::open_async("", target.into(), String::new()),
        Do::Lock => {
            let _ = unsafe { LockWorkStation() };
        }
        Do::Sleep => {
            std::thread::spawn(|| unsafe {
                let _ = SetSuspendState(false, false, false);
            });
        }
        Do::SignOut => shutdown("/l"),
        Do::SwitchUser => {
            // Leaves everything running and goes to the sign-in screen.
            std::thread::spawn(|| unsafe {
                use windows::Win32::System::RemoteDesktop::{WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTSDisconnectSession};
                let _ = WTSDisconnectSession(Some(WTS_CURRENT_SERVER_HANDLE), WTS_CURRENT_SESSION, false);
            });
        }
        Do::Restart => shutdown("/r /t 0"),
        Do::ShutDown => shutdown("/s /t 0"),
        Do::EmptyRecycleBin => {
            std::thread::spawn(|| unsafe {
                let _ = SHEmptyRecycleBinW(None, PCWSTR::null(), 0);
            });
        }
        Do::Customize | Do::PinPage | Do::RealStart | Do::Quit => {}
    }
}
