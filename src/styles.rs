//! The looks the whole app can take. The Windows ones are fixed, like the real thing; only
//! Startline has free colors, shapes and placement.

use crate::config::{Config, CornerKind, HAnchor, Theme, VAnchor};
use crate::i18n::tx;
use crate::system::Placement;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
pub enum Experience {
    #[default]
    Windows11,
    Windows10,
    Windows7,
    WindowsXp,
    Startline,
}

impl Experience {
    pub const ALL: [Experience; 5] = [Experience::Windows11, Experience::Windows10, Experience::Windows7, Experience::WindowsXp, Experience::Startline];

    pub fn label(self) -> &'static str {
        match self {
            Experience::Windows11 => tx("Windows 11"),
            Experience::Windows10 => tx("Windows 10"),
            Experience::Windows7 => tx("Windows 7"),
            Experience::WindowsXp => tx("Windows XP"),
            Experience::Startline => "Startline", // i18n-ignore: the app's name
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Experience::Windows11 => tx("Centered and rounded, with pinned apps on top. Follows your light or dark setting."),
            Experience::Windows10 => tx("App list and tiles, flush with the taskbar. Follows your light or dark setting."),
            Experience::Windows7 => tx("White program list beside a glass panel."),
            Experience::WindowsXp => tx("The blue and white two-column menu."),
            Experience::Startline => tx("Our own look. Pick the colors, shape, size and position yourself."),
        }
    }

    pub fn from_name(name: &str) -> Option<Experience> {
        let wanted = name.to_lowercase().replace([' ', '-'], "");
        Experience::ALL.into_iter().find(|e| {
            let label = e.label().to_lowercase().replace(' ', "");
            label == wanted || label.replace("windows", "win") == wanted || label.replace("windows", "") == wanted
        })
    }

    /// Controls are drawn squarer and menus light up under the pointer in the older looks.
    pub fn is_classic(self) -> bool {
        matches!(self, Experience::Windows7 | Experience::WindowsXp)
    }
}

/// What Windows itself is set to right now.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SystemLook {
    pub dark: bool,
    pub accent: [u8; 3],
}

impl Default for SystemLook {
    fn default() -> Self {
        SystemLook { dark: true, accent: [0, 120, 212] }
    }
}

impl SystemLook {
    pub fn read() -> SystemLook {
        use crate::settings::reg_dword;
        const PERSONALIZE: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
        let accent = reg_dword("Software\\Microsoft\\Windows\\DWM", "AccentColor")
            .map(|abgr| [(abgr & 0xFF) as u8, (abgr >> 8 & 0xFF) as u8, (abgr >> 16 & 0xFF) as u8])
            .unwrap_or(SystemLook::default().accent);
        SystemLook {
            dark: reg_dword(PERSONALIZE, "AppsUseLightTheme").is_none_or(|v| v == 0),
            accent,
        }
    }
}

/// Rounding that Windows does itself. Glass ignores any shape cut by the app, so a window with
/// glass has to use this, or a square corner of glass shows behind the rounded one.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum NativeCorner {
    Round,
    Small,
}

impl NativeCorner {
    /// In screen pixels, measured from real windows: Windows doesn't scale it with display size.
    pub fn pixels(self) -> f32 {
        match self {
            NativeCorner::Round => 8.0,
            NativeCorner::Small => 4.0,
        }
    }
}

/// Everything about how the window is drawn, worked out from the chosen experience.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Look {
    pub experience: Experience,
    pub theme: Theme,
    pub opacity: f32,
    pub blur: bool,
    /// A corner cut by the app, in points, for a look without glass.
    pub corner: f32,
    /// The older menus are rounded on top and square at the bottom.
    pub round_bottom: bool,
    pub native_corner: Option<NativeCorner>,
    pub ui_scale: f32,
    /// Mica behind the menu instead of frosted glass, and whether a lighter shade of it shows through the search box
    /// and the selected item.
    pub mica: bool,
    pub mica_light: bool,
}

impl Look {
    /// The window's corner in points, so the paint lines up exactly with the window's edge.
    pub fn corner_points(&self, pixels_per_point: f32) -> f32 {
        self.native_corner.map_or(self.corner, |native| native.pixels() / pixels_per_point.max(0.1))
    }
}

pub fn look_of(cfg: &Config, system: SystemLook) -> Look {
    let experience = cfg.experience;
    let accent = system.accent;
    let shaded = |bg, surface, text, subtext| Theme { bg, surface, text, subtext, accent };
    let base = Look {
        experience,
        theme: crate::config::theme_of(cfg, system.dark),
        opacity: 1.0,
        blur: false,
        corner: 0.0,
        round_bottom: true,
        native_corner: None,
        ui_scale: cfg.ui_scale,
        mica: false,
        mica_light: false,
    };
    let look = match experience {
        Experience::Windows11 => Look {
            theme: if system.dark {
                shaded([32, 32, 32], [50, 50, 50], [255, 255, 255], [171, 171, 171])
            } else {
                shaded([243, 243, 243], [251, 251, 251], [27, 27, 27], [96, 96, 96])
            },
            opacity: 0.88,
            blur: true,
            native_corner: Some(NativeCorner::Round),
            ..base
        },
        Experience::Windows10 => Look {
            theme: if system.dark {
                shaded([31, 31, 31], [52, 52, 52], [255, 255, 255], [170, 170, 170])
            } else {
                shaded([230, 230, 230], [246, 246, 246], [0, 0, 0], [90, 90, 90])
            },
            opacity: 0.9,
            blur: true,
            ..base
        },
        Experience::Windows7 => Look {
            theme: Theme { bg: [255, 255, 255], surface: [240, 244, 249], text: [0, 0, 0], subtext: [84, 84, 84], accent: [51, 153, 255] },
            blur: true,
            native_corner: Some(NativeCorner::Small),
            ..base
        },
        Experience::WindowsXp => Look {
            theme: Theme { bg: [255, 255, 255], surface: [236, 233, 216], text: [0, 0, 0], subtext: [84, 84, 84], accent: [49, 106, 197] },
            corner: 7.0,
            round_bottom: false,
            ..base
        },
        Experience::Startline => Look {
            opacity: cfg.opacity,
            blur: cfg.blur,
            native_corner: match cfg.corners {
                CornerKind::Round => Some(NativeCorner::Round),
                CornerKind::Slight => Some(NativeCorner::Small),
                CornerKind::Square => None,
            },
            ..base
        },
    };
    // Mica is Windows 11's own, so only its look and Startline's take it, and only where Windows has it.
    if cfg.mica && matches!(experience, Experience::Windows11 | Experience::Startline) && crate::platform::has_mica() {
        return Look { mica: true, mica_light: cfg.mica_light, blur: false, ..look };
    }
    look
}

/// Only for pictures of the settings pages, which are otherwise a window of their own.
const SETTINGS_SIZE: [f32; 2] = [1040.0, 720.0];

pub fn placement_of(cfg: &Config, _system: SystemLook, in_settings: bool) -> Placement {
    if in_settings {
        return Placement { h: HAnchor::Center, v: VAnchor::Middle, width: SETTINGS_SIZE[0], height: SETTINGS_SIZE[1], margin: 0.0, on_top: false, taskbar: false };
    }
    // Against the taskbar where Windows' own menu would be, with the gap that version of Windows has; or, chosen in
    // Settings, the spot on the screen picked there.
    let place = |width, height, gap| {
        if cfg.align_to_taskbar {
            Placement { h: HAnchor::Center, v: VAnchor::Bottom, width, height, margin: gap, on_top: true, taskbar: true }
        } else {
            Placement { h: cfg.h_anchor, v: cfg.v_anchor, width, height, margin: cfg.margin, on_top: true, taskbar: false }
        }
    };
    // The calendar panel makes the menu wider, only while it's switched on.
    let panel = if crate::m365::panel_shown(cfg) { crate::m365::PANEL_WIDTH } else { 0.0 };
    match cfg.experience {
        Experience::Windows11 => place(660.0 + panel, 720.0, 12.0),
        Experience::Windows10 => place(660.0, 640.0, 0.0),
        Experience::Windows7 => place(430.0, 560.0, 0.0),
        Experience::WindowsXp => place(420.0, 540.0, 0.0),
        Experience::Startline => place(cfg.width + panel, cfg.height, cfg.margin),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_11_is_the_default_and_paints_the_corner_windows_gives_it() {
        let cfg = Config { roots: vec![], ..Default::default() };
        assert_eq!(cfg.experience, Experience::Windows11);
        let look = look_of(&cfg, SystemLook::default());
        assert_eq!(look.native_corner, Some(NativeCorner::Round));
        assert_eq!(look.corner_points(1.25), 8.0 / 1.25, "the same screen pixels at any display size");
    }

    #[test]
    fn every_look_with_glass_lets_windows_do_the_rounding() {
        let mut cfg = Config { roots: vec![], ..Default::default() };
        for experience in Experience::ALL {
            cfg.experience = experience;
            let look = look_of(&cfg, SystemLook::default());
            if look.blur {
                assert!(look.native_corner.is_some() || look.corner == 0.0, "{experience:?} would show a square corner of glass");
            }
        }
    }

    #[test]
    fn custom_colors_only_reach_the_startline_look() {
        let mut cfg = Config { roots: vec![], preset: "Custom".into(), ..Default::default() };
        cfg.theme.bg = [1, 2, 3];
        for experience in [Experience::Windows11, Experience::Windows10, Experience::Windows7, Experience::WindowsXp] {
            cfg.experience = experience;
            assert_ne!(look_of(&cfg, SystemLook::default()).theme.bg, [1, 2, 3]);
        }
        cfg.experience = Experience::Startline;
        assert_eq!(look_of(&cfg, SystemLook::default()).theme.bg, [1, 2, 3]);
    }

    #[test]
    fn windows_11_and_10_follow_light_and_dark() {
        let mut cfg = Config { roots: vec![], ..Default::default() };
        let light = SystemLook { dark: false, ..Default::default() };
        for experience in [Experience::Windows11, Experience::Windows10] {
            cfg.experience = experience;
            assert_ne!(look_of(&cfg, light).theme.bg, look_of(&cfg, SystemLook::default()).theme.bg);
        }
    }

    #[test]
    fn names_on_the_command_line_are_forgiving() {
        assert_eq!(Experience::from_name("win7"), Some(Experience::Windows7));
        assert_eq!(Experience::from_name("Windows XP"), Some(Experience::WindowsXp));
        assert_eq!(Experience::from_name("startline"), Some(Experience::Startline));
        assert_eq!(Experience::from_name("nope"), None);
    }
}
