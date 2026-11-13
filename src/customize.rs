//! The Settings window: a normal window with a title bar, a list of pages on the left and
//! pages laid out to use the room they have. Every change shows at once and saves on its own.

use crate::actions::Do;
use crate::config::{Config, CornerKind, HAnchor, PRESETS, Section, SectionKind, VAnchor, data_dir};
use crate::i18n::{tr, tx};
use crate::trf;
use crate::platform;
use crate::settings;
use crate::styles::Experience;
use crate::ui::{Intent, Menu, Page};
use crate::widgets::*;
use egui::*;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tab {
    Appearance,
    StartMenu,
    Pins,
    Search,
    General,
    Microsoft365,
    About,
}

impl Tab {
    const ALL: [(Tab, &'static str, char); 7] = [
        (Tab::Appearance, tx("Appearance"), '\u{E790}'),
        (Tab::StartMenu, tx("Start menu"), '\u{E8FC}'),
        (Tab::Pins, tx("Pinned apps"), '\u{E718}'),
        (Tab::Search, tx("Search"), '\u{E721}'),
        (Tab::General, tx("General"), '\u{E713}'),
        (Tab::Microsoft365, tx("Microsoft 365"), '\u{E753}'),
        (Tab::About, tx("About & updates"), '\u{E946}'),
    ];

    pub fn from_name(name: &str) -> Option<Tab> {
        Tab::ALL.iter().find(|(_, label, _)| label.to_lowercase().starts_with(&name.to_lowercase())).map(|(tab, _, _)| *tab)
    }
}

pub(crate) const WINDOW_TITLE: &str = tx("Startline Settings");

pub fn show_window(menu: &mut Menu, ctx: &Context) {
    let id = ViewportId::from_hash_of("startline-settings");
    let (size, position) = menu.settings_start;
    let icon = IconData { rgba: crate::app_icon_pixels(64), width: 64, height: 64 };
    // One title in the language shown, which is also how the title bar is found to darken it.
    let title = tr(WINDOW_TITLE);
    let mut builder = ViewportBuilder::default()
        .with_title(title)
        .with_inner_size(size)
        .with_min_inner_size([720.0, 480.0])
        .with_icon(icon)
        .with_taskbar(true);
    if let Some(position) = position {
        builder = builder.with_position(position);
    }
    if std::mem::take(&mut menu.settings_raise) {
        ctx.send_viewport_cmd_to(id, ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd_to(id, ViewportCommand::Focus);
    }
    ctx.show_viewport_immediate(id, builder, |ui, _class| {
        menu.drawing_settings = true;
        let pal = menu.pal();
        let inner_ctx = ui.ctx().clone();
        ui.painter().rect_filled(ui.max_rect(), 0, pal.bg);
        let tab = menu.settings_tab;
        draw(menu, ui, tab);
        menu.draw_popup(&inner_ctx, pal);
        platform::set_title_bar_dark(title, luminance_of(pal.bg) < 0.5);

        let (close, inner, outer, maximized) =
            inner_ctx.input(|i| (i.viewport().close_requested(), i.viewport().inner_rect, i.viewport().outer_rect, i.viewport().maximized.unwrap_or(false)));
        if !maximized {
            if let Some(inner) = inner {
                menu.cfg.settings_size = [inner.width().round(), inner.height().round()];
            }
            if let Some(outer) = outer {
                menu.cfg.settings_pos = Some([outer.min.x.round(), outer.min.y.round()]);
            }
        }
        if close {
            menu.settings_open = false;
            menu.popup = None;
            menu.sync_index();
        }
        menu.drawing_settings = false;
    });
}

fn luminance_of(color: Color32) -> f32 {
    (0.299 * color.r() as f32 + 0.587 * color.g() as f32 + 0.114 * color.b() as f32) / 255.0
}

pub fn draw(menu: &mut Menu, ui: &mut Ui, tab: Tab) {
    let pal = menu.pal();
    let full = ui.max_rect();
    let sidebar = Rect::from_min_max(full.min, pos2(full.left() + 196.0, full.bottom()));
    ui.painter().rect_filled(sidebar, 0, mix(pal.bg, pal.text, 0.03));
    ui.painter().vline(sidebar.right(), sidebar.y_range(), Stroke::new(1.0, pal.line));

    let mut side = ui.new_child(UiBuilder::new().max_rect(sidebar.shrink2(vec2(10.0, 14.0))).layout(Layout::top_down(Align::Min)));
    for (candidate, label, glyph) in Tab::ALL {
        let (rect, response) = side.allocate_exact_size(vec2(side.available_width(), 36.0), Sense::click());
        let active = candidate == tab;
        if active {
            side.painter().rect_filled(rect, pal.radius, pal.selected);
            let bar = Rect::from_center_size(pos2(rect.left() + 2.0, rect.center().y), vec2(3.0, 16.0));
            side.painter().rect_filled(bar, 1, pal.accent);
        } else if response.hovered() {
            side.painter().rect_filled(rect, pal.radius, pal.hover);
        }
        let ink = if active && pal.style == Experience::WindowsXp { pal.on_accent } else { pal.text };
        side.painter().text(pos2(rect.left() + 20.0, rect.center().y), Align2::CENTER_CENTER, glyph, icon_font(15.0), ink);
        side.painter().text(pos2(rect.left() + 40.0, rect.center().y - ink_nudge(13.5)), Align2::LEFT_CENTER, tr(label), FontId::proportional(13.5), ink);
        if response.clicked() {
            menu.intents.push(Intent::Show(Page::Customize(candidate)));
        }
    }

    let body = Rect::from_min_max(pos2(sidebar.right() + 1.0, full.top()), full.max);
    let mut body_ui = ui.new_child(UiBuilder::new().max_rect(body).layout(Layout::top_down(Align::Min)));
    body_ui.set_clip_rect(body);
    let mut scroll = ScrollArea::vertical().id_salt(("settings", tab as u8)).auto_shrink([false, false]);
    if let (Some(down), true) = (menu.options.demo_scroll, menu.is_shot()) {
        scroll = scroll.vertical_scroll_offset(down);
    }
    scroll.show(&mut body_ui, |ui| {
        // Comfortable margins, and no lines stretching across a huge window.
        let room = ui.available_width();
        let width = (room - 56.0).clamp(380.0, 1120.0);
        ui.add_space(20.0);
        ui.horizontal_top(|ui| {
            ui.add_space(((room - width) / 2.0).max(16.0));
            ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(width);
                if menu.policy.on_page(tab) {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new('\u{E72E}'.to_string()).font(icon_font(13.0)).color(pal.sub));
                        ui.label(RichText::new(tr("Your organization manages some of this page, so those settings are dimmed or removed.")).size(12.5).color(pal.sub));
                    });
                    ui.add_space(10.0);
                }
                match tab {
                    Tab::Appearance => appearance(menu, ui, pal),
                    Tab::StartMenu => start_menu(menu, ui, pal),
                    Tab::Pins => crate::pins_page::draw(menu, ui, pal),
                    Tab::Search => crate::search_page::draw(menu, ui, pal),
                    Tab::General => general(menu, ui, pal),
                    Tab::Microsoft365 => crate::m365::page::draw(menu, ui, pal),
                    Tab::About => about(menu, ui, pal),
                }
            });
        });
        ui.add_space(24.0);
    });
}

/// The big title of a page and what it's for.
pub(crate) fn page_header(ui: &mut Ui, title: &str, about: &str, pal: Pal) {
    ui.label(RichText::new(title).font(semibold(22.0)).color(pal.text));
    ui.add_space(2.0);
    ui.label(RichText::new(about).size(13.0).color(pal.sub));
    ui.add_space(14.0);
}

pub(crate) fn heading(ui: &mut Ui, text: &str, pal: Pal) {
    ui.add_space(4.0);
    ui.label(RichText::new(text).font(semibold(13.5)).color(pal.text));
    ui.add_space(4.0);
}

/// A group of settings on one raised panel.
pub(crate) fn card(ui: &mut Ui, pal: Pal, add: impl FnOnce(&mut Ui)) {
    Frame::new().fill(pal.surface).stroke(Stroke::new(1.0, pal.line)).corner_radius(pal.radius + 2).inner_margin(12).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 8.0;
        add(ui);
    });
    ui.add_space(10.0);
}

/// What a setting is and what it does on the left, its control on the right.
fn row<R>(ui: &mut Ui, title: &str, about: &str, pal: Pal, control: impl FnOnce(&mut Ui) -> R) -> R {
    let room = ui.available_width() * 0.45;
    row_with_room(ui, title, about, room, pal, control)
}

/// `room` is how wide the words may get before they wrap.
fn row_with_room<R>(ui: &mut Ui, title: &str, about: &str, room: f32, pal: Pal, control: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.set_min_height(34.0);
        // The words keep to their side and wrap there, so a long title in German or Italian never runs under the control.
        // A short title on its own stays one line, centered beside the control.
        let fits = ui.painter().layout_no_wrap(title.to_string(), FontId::proportional(13.5), pal.text).size().x <= room;
        if about.is_empty() && fits {
            ui.label(RichText::new(title).size(13.5).color(pal.text));
        } else {
            ui.vertical(|ui| {
                ui.set_max_width(room);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add(Label::new(RichText::new(title).size(13.5).color(pal.text)).wrap());
                if !about.is_empty() {
                    ui.add(Label::new(RichText::new(about).size(12.0).color(pal.sub)).wrap());
                }
            });
        }
        ui.with_layout(Layout::right_to_left(Align::Center), control).inner
    })
    .inner
}

/// A switch is small, so its words may use the row up to it.
fn switch_row(ui: &mut Ui, title: &str, about: &str, value: &mut bool, pal: Pal) -> bool {
    let room = ui.available_width() - 70.0;
    row_with_room(ui, title, about, room, pal, |ui| switch(ui, value, pal).changed())
}

fn slider(ui: &mut Ui, title: &str, about: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, pal: Pal) {
    row(ui, title, about, pal, |ui| {
        ui.spacing_mut().slider_width = 130.0;
        ui.add(Slider::new(value, range).max_decimals(0).text_color(pal.text));
    });
}

fn note(ui: &mut Ui, text: &str, pal: Pal) {
    ui.label(RichText::new(text).size(12.5).color(pal.sub));
}

/// A tiny drawing of what each look's menu is like.
fn style_preview(ui: &Ui, rect: Rect, style: Experience) {
    let painter = ui.painter();
    let inner = rect.shrink(8.0);
    let w = inner.width();
    let h = inner.height();
    let part = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(inner.min + vec2(w * x0, h * y0), inner.min + vec2(w * x1, h * y1));
    match style {
        Experience::Windows11 => {
            painter.rect_filled(rect, 4, Color32::from_rgb(70, 112, 170));
            let menu = part(0.2, 0.08, 0.8, 1.0);
            painter.rect_filled(menu, 4, Color32::from_rgb(38, 38, 38));
            painter.rect_filled(part(0.26, 0.15, 0.74, 0.25), 3, Color32::from_rgb(70, 70, 70));
            for i in 0..4 {
                let x = 0.27 + i as f32 * 0.12;
                painter.rect_filled(part(x, 0.36, x + 0.08, 0.5), 1, Color32::from_rgb(120, 170, 230));
            }
        }
        Experience::Windows10 => {
            painter.rect_filled(rect, 0, Color32::from_rgb(40, 76, 120));
            painter.rect_filled(part(0.0, 0.08, 0.7, 1.0), 0, Color32::from_rgb(31, 31, 31));
            painter.rect_filled(part(0.0, 0.08, 0.08, 1.0), 0, Color32::from_rgb(48, 48, 48));
            for i in 0..2 {
                for j in 0..3 {
                    let (x, y) = (0.36 + i as f32 * 0.16, 0.16 + j as f32 * 0.26);
                    painter.rect_filled(part(x, y, x + 0.14, y + 0.22), 0, Color32::from_rgb(0, 120, 212));
                }
            }
        }
        Experience::Windows7 => {
            painter.rect_filled(rect, 3, Color32::from_rgb(46, 104, 150));
            painter.rect_filled(part(0.0, 0.05, 0.72, 1.0), 4, Color32::from_rgb(70, 126, 170));
            painter.rect_filled(part(0.03, 0.09, 0.44, 0.84), 2, Color32::WHITE);
            painter.rect_filled(part(0.03, 0.87, 0.44, 0.97), 2, Color32::from_rgb(235, 240, 246));
        }
        Experience::WindowsXp => {
            painter.rect_filled(rect, 3, Color32::from_rgb(58, 110, 165));
            painter.rect_filled(part(0.0, 0.03, 0.74, 0.2), 3, Color32::from_rgb(36, 112, 220));
            painter.rect_filled(part(0.0, 0.2, 0.42, 0.88), 0, Color32::WHITE);
            painter.rect_filled(part(0.42, 0.2, 0.74, 0.88), 0, Color32::from_rgb(211, 229, 250));
            painter.rect_filled(part(0.0, 0.88, 0.74, 1.0), 0, Color32::from_rgb(36, 112, 220));
        }
        Experience::Startline => {
            let [r, g, b] = PRESETS[0].1.bg;
            painter.rect_filled(rect, 6, Color32::from_rgb(r, g, b));
            let [r, g, b] = PRESETS[0].1.surface;
            painter.rect_filled(part(0.08, 0.84, 0.92, 0.98), 3, Color32::from_rgb(r, g, b));
            let [r, g, b] = PRESETS[0].1.subtext;
            for i in 0..5 {
                let x = 0.1 + i as f32 * 0.17;
                painter.rect_filled(part(x, 0.14, x + 0.11, 0.34), 2, Color32::from_rgb(r, g, b));
            }
        }
    }
}

fn appearance(menu: &mut Menu, ui: &mut Ui, pal: Pal) {
    page_header(ui, tr("Appearance"), tr("Choose how the whole app looks: the Start menu, search and this window."), pal);
    let policy = menu.policy.clone();
    two_columns(menu, ui, |menu, ui| {
        heading(ui, tr("Look"), pal);
        let columns = if ui.available_width() > 420.0 { 3 } else { 2 };
        let gap = 8.0;
        let width = (ui.available_width() - gap * (columns as f32 - 1.0)) / columns as f32;
        policy.lock(ui, &["Experience"], |ui| for chunk in Experience::ALL.chunks(columns) { // i18n-ignore: policy name
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(gap, 0.0);
                for &style in chunk {
                    let (rect, response) = ui.allocate_exact_size(vec2(width, width * 0.62 + 30.0), Sense::click());
                    let chosen = menu.cfg.experience == style;
                    ui.painter().rect_filled(rect, pal.radius + 2, if response.hovered() { pal.hover } else { pal.surface });
                    let stroke = if chosen { Stroke::new(2.0, pal.accent) } else { Stroke::new(1.0, pal.line) };
                    ui.painter().rect_stroke(rect, pal.radius + 2, stroke, StrokeKind::Inside);
                    let picture = Rect::from_min_size(rect.min + vec2(7.0, 7.0), vec2(rect.width() - 14.0, width * 0.62 - 6.0));
                    style_preview(ui, picture, style);
                    ui.painter().text(pos2(rect.left() + 10.0, rect.bottom() - 14.0), Align2::LEFT_CENTER, tr(style.label()), semibold(12.5), pal.text);
                    let response = response.on_hover_text(tr(style.blurb()));
                    if response.clicked() {
                        menu.cfg.experience = style;
                    }
                }
            });
            ui.add_space(gap);
        });
        note(ui, tr(menu.cfg.experience.blurb()), pal);
        ui.add_space(12.0);
        heading(ui, tr("Presets"), pal);
        card(ui, pal, |ui| {
            note(ui, tr("Sets the look and the settings that go with it in one go, as if you changed each by hand. Each stays yours to change afterwards."), pal);
            ui.horizontal_wrapped(|ui| {
                for preset in Preset::ALL {
                    if pill_button(ui, tr(preset.label()), pal, false).on_hover_text(tr(preset.about())).clicked() {
                        preset.apply(&mut menu.cfg);
                    }
                }
            });
        });
    }, |menu, ui| {
        heading(ui, tr("Language"), pal);
        card(ui, pal, |ui| {
            policy.lock(ui, &["Language"], |ui| row(ui, tr("Language"), "", pal, |ui| { // i18n-ignore: policy name
                let windows_language = crate::i18n::CHOICES.iter().find(|(c, _)| *c == crate::i18n::system().code()).map_or("English", |(_, n)| *n); // i18n-ignore: a language's own name
                let name = |code: &str| match code {
                    "system" => trf!("Windows language ({language})", language = windows_language),
                    other => crate::i18n::CHOICES.iter().find(|(c, _)| *c == other).map_or(other, |(_, n)| *n).to_string(),
                };
                ComboBox::from_id_salt("language").width(200.0).icon(|ui, rect, visuals, open| chevron(ui, rect, open, true, visuals.fg_stroke.color)).selected_text(name(&menu.cfg.language)).show_ui(ui, |ui| {
                    for (code, _) in crate::i18n::CHOICES {
                        ui.selectable_value(&mut menu.cfg.language, code.to_string(), name(code));
                    }
                });
            }));
        });
        heading(ui, tr("Size"), pal);
        card(ui, pal, |ui| {
            let sizes = [0.9, 1.0, 1.15];
            let now = sizes.iter().position(|s: &f32| (*s - menu.cfg.ui_scale).abs() < 0.05).unwrap_or(1);
            policy.lock(ui, &["TextAndIconSize"], |ui| row(ui, tr("Text and icons"), "", pal, |ui| {
                if let Some(i) = segmented(ui, "ui-size", &[tr("Small"), tr("Normal"), tr("Large")], now, 210.0, pal) {
                    menu.cfg.ui_scale = sizes[i];
                }
            }));
        });
        // Mica is Windows 11's own: offered where Windows has it, for the looks that can take it.
        if platform::has_mica() && matches!(menu.cfg.experience, Experience::Windows11 | Experience::Startline) {
            heading(ui, tr("Mica"), pal);
            card(ui, pal, |ui| {
                policy.lock(ui, &["Mica"], |ui| switch_row(ui, tr("Mica behind the Start menu"), tr("Windows 11's material, tinted by your desktop picture, instead of frosted glass."), &mut menu.cfg.mica, pal)); // i18n-ignore: policy name
                let mica = menu.cfg.mica;
                ui.indent("mica-parts", |ui| {
                    ui.add_enabled_ui(mica, |ui| {
                        policy.lock(ui, &["MicaLight"], |ui| switch_row(ui, tr("Lighter Mica"), tr("Through the search box and the selected item."), &mut menu.cfg.mica_light, pal));
                    });
                });
            });
        }
        if menu.cfg.experience != Experience::Startline {
            return;
        }
        heading(ui, tr("Colors"), pal);
        let columns = 4;
        let gap = 6.0;
        let width = (ui.available_width() - gap * (columns as f32 - 1.0)) / columns as f32;
        policy.lock(ui, &crate::policy::THEME_KEYS, |ui| {
        for chunk in PRESETS.chunks(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(gap, 0.0);
                for (name, theme) in chunk {
                    let (rect, response) = ui.allocate_exact_size(vec2(width, 46.0), Sense::click());
                    let [r, g, b] = theme.bg;
                    ui.painter().rect_filled(rect, pal.radius, Color32::from_rgb(r, g, b));
                    let stroke = if menu.cfg.preset == *name { Stroke::new(2.0, pal.accent) } else { Stroke::new(1.0, pal.line) };
                    ui.painter().rect_stroke(rect, pal.radius, stroke, StrokeKind::Inside);
                    let [ar, ag, ab] = theme.accent;
                    ui.painter().circle_filled(rect.right_top() + vec2(-12.0, 12.0), 4.0, Color32::from_rgb(ar, ag, ab));
                    let [ink_r, ink_g, ink_b] = theme.text;
                    ui.painter().text(pos2(rect.left() + 8.0, rect.bottom() - 12.0), Align2::LEFT_CENTER, tr(*name), FontId::proportional(12.0), Color32::from_rgb(ink_r, ink_g, ink_b));
                    if response.clicked() {
                        menu.cfg.theme = *theme;
                        menu.cfg.preset = name.to_string();
                    }
                }
            });
            ui.add_space(gap);
        }
        CollapsingHeader::new(RichText::new(tr("Pick each color yourself")).size(13.0).color(pal.text)).icon(|ui, openness, response| chevron(ui, response.rect, openness > 0.5, false, ui.visuals().text_color())).show(ui, |ui| {
            let before = menu.cfg.theme;
            {
                let theme = &mut menu.cfg.theme;
                for (label, value) in [
                    (tr("Background"), &mut theme.bg),
                    (tr("Panels"), &mut theme.surface),
                    (tr("Text"), &mut theme.text),
                    (tr("Quiet text"), &mut theme.subtext),
                    (tr("Accent"), &mut theme.accent),
                ] {
                    row(ui, label, "", pal, |ui| {
                        ui.color_edit_button_srgb(value);
                    });
                }
            }
            if menu.cfg.theme != before {
                menu.cfg.preset = "Custom".into(); // i18n-ignore: saved name of an own theme
            }
        });
        });
        ui.add_space(8.0);
        heading(ui, tr("Window"), pal);
        card(ui, pal, |ui| {
            let kinds = [CornerKind::Round, CornerKind::Slight, CornerKind::Square];
            let now = kinds.iter().position(|k| *k == menu.cfg.corners).unwrap_or(0);
            policy.lock(ui, &["Corners"], |ui| row(ui, tr("Corners"), "", pal, |ui| { // i18n-ignore: policy name
                if let Some(i) = segmented(ui, "corners", &[tr("Round"), tr("Slight"), tr("Square")], now, 210.0, pal) {
                    menu.cfg.corners = kinds[i];
                }
            }));
            policy.lock(ui, &["FrostedGlass"], |ui| switch_row(ui, tr("Frosted glass"), tr("Blurs what's behind the menu."), &mut menu.cfg.blur, pal));
            policy.lock(ui, &["Opacity"], |ui| row(ui, tr("See-through"), "", pal, |ui| { // i18n-ignore: policy name
                ui.spacing_mut().slider_width = 130.0;
                ui.add(Slider::new(&mut menu.cfg.opacity, 0.5..=1.0).max_decimals(2).text_color(pal.text));
            }));
        });
    });
}

/// A look set in one go. Not a setting of its own: it changes the settings it covers once, as if by hand, and each
/// of them keeps its own place in Settings.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Preset {
    Windows11,
    Startline,
    Compact,
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::Windows11, Preset::Startline, Preset::Compact];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Windows11 => tx("Look like Windows 11 Start"),
            Preset::Startline => tx("Startline's own look"),
            Preset::Compact => tx("Compact"),
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Preset::Windows11 => tx("The Windows 11 look, with Mica where Windows has it, pinned apps and recent files."),
            Preset::Startline => tx("Startline's own look: Stone colors, frosted glass, round corners and search at the bottom."),
            Preset::Compact => tx("Startline's look, smaller: tighter lists, smaller pinned apps without names, and fewer recent files."),
        }
    }

    pub fn apply(self, cfg: &mut Config) {
        let sections = |recent: bool| {
            [(SectionKind::Pinned, true), (SectionKind::Recent, recent), (SectionKind::Frequent, false), (SectionKind::AllApps, false)].into_iter().map(|(kind, visible)| Section { kind, visible }).collect::<Vec<_>>()
        };
        match self {
            Preset::Windows11 => {
                cfg.experience = Experience::Windows11;
                cfg.mica = platform::has_mica();
                cfg.mica_light = true;
                cfg.ui_scale = 1.0;
                cfg.sections = sections(true);
                cfg.recent_count = 6;
                cfg.compact_rows = false;
            }
            Preset::Startline | Preset::Compact => {
                let compact = self == Preset::Compact;
                cfg.experience = Experience::Startline;
                cfg.preset = PRESETS[0].0.to_string();
                cfg.theme = PRESETS[0].1;
                cfg.corners = CornerKind::Round;
                cfg.mica = false;
                cfg.blur = true;
                cfg.opacity = 0.96;
                cfg.search_at_bottom = true;
                cfg.h_anchor = HAnchor::Center;
                cfg.v_anchor = VAnchor::Bottom;
                cfg.margin = 12.0;
                cfg.sections = sections(true);
                cfg.ui_scale = if compact { 0.9 } else { 1.0 };
                cfg.columns = if compact { 8 } else { 6 };
                cfg.tile_size = if compact { 60.0 } else { 84.0 };
                cfg.show_labels = !compact;
                cfg.compact_rows = compact;
                cfg.recent_count = if compact { 4 } else { 6 };
                cfg.width = if compact { 540.0 } else { 660.0 };
                cfg.height = if compact { 580.0 } else { 720.0 };
            }
        }
    }
}

fn start_menu(menu: &mut Menu, ui: &mut Ui, pal: Pal) {
    page_header(ui, tr("Start menu"), tr("What the menu shows, and for the Startline look, where and how it opens."), pal);
    let policy = menu.policy.clone();
    two_columns(menu, ui, |menu, ui| {
        heading(ui, tr("What it shows"), pal);
        policy.lock(ui, &["Sections"], |ui| card(ui, pal, |ui| { // i18n-ignore: policy name
            drag_rows(ui, Id::unique("start-sections"), &mut menu.cfg.sections, 34.0, pal, |ui, section: &mut Section| {
                ui.label(RichText::new(section.kind.label()).size(13.5).color(pal.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    switch(ui, &mut section.visible, pal);
                });
            });
        }));
        note(ui, tr("Drag the handles to change the order."), pal);
        ui.add_space(10.0);
        heading(ui, tr("How much"), pal);
        card(ui, pal, |ui| {
            let mut count = menu.cfg.recent_count as f32;
            policy.lock(ui, &["RecentCount"], |ui| slider(ui, tr("Recent files"), "", &mut count, 2.0..=20.0, pal));
            menu.cfg.recent_count = count as usize;
            let mut count = menu.cfg.frequent_count as f32;
            policy.lock(ui, &["FrequentCount"], |ui| slider(ui, tr("Most-used items"), "", &mut count, 2.0..=20.0, pal));
            menu.cfg.frequent_count = count as usize;
        });
    }, |menu, ui| {
        if pal.style != Experience::Startline {
            heading(ui, tr("Size and position"), pal);
            card(ui, pal, |ui| {
                note(ui, tr("This look opens where that version of Windows did. Choose the Startline look under Appearance to place it freely."), pal);
            });
            return;
        }
        heading(ui, tr("Layout"), pal);
        card(ui, pal, |ui| {
            policy.lock(ui, &["SearchAtBottom"], |ui| switch_row(ui, tr("Search bar at the bottom"), "", &mut menu.cfg.search_at_bottom, pal));
            let mut columns = menu.cfg.columns as f32;
            policy.lock(ui, &["Columns"], |ui| slider(ui, tr("Pinned apps per row"), "", &mut columns, 3.0..=12.0, pal)); // i18n-ignore: policy name
            menu.cfg.columns = columns as usize;
            policy.lock(ui, &["TileSize"], |ui| slider(ui, tr("Pinned app size"), "", &mut menu.cfg.tile_size, 52.0..=132.0, pal));
            policy.lock(ui, &["ShowLabels"], |ui| switch_row(ui, tr("Names under pinned apps"), "", &mut menu.cfg.show_labels, pal));
            policy.lock(ui, &["CompactRows"], |ui| switch_row(ui, tr("Compact lists"), "", &mut menu.cfg.compact_rows, pal));
        });
        heading(ui, tr("Where it opens"), pal);
        card(ui, pal, |ui| {
            let h_options = [HAnchor::Left, HAnchor::Center, HAnchor::Right];
            let h_now = h_options.iter().position(|a| *a == menu.cfg.h_anchor).unwrap_or(1);
            policy.lock(ui, &["HorizontalPosition"], |ui| row(ui, tr("Across"), "", pal, |ui| {
                if let Some(i) = segmented(ui, "h-anchor", &[tr("Left"), tr("Center"), tr("Right")], h_now, 210.0, pal) {
                    menu.cfg.h_anchor = h_options[i];
                }
            }));
            let v_options = [VAnchor::Top, VAnchor::Middle, VAnchor::Bottom];
            let v_now = v_options.iter().position(|a| *a == menu.cfg.v_anchor).unwrap_or(2);
            policy.lock(ui, &["VerticalPosition"], |ui| row(ui, tr("Up and down"), "", pal, |ui| {
                if let Some(i) = segmented(ui, "v-anchor", &[tr("Top"), tr("Middle"), tr("Bottom")], v_now, 210.0, pal) {
                    menu.cfg.v_anchor = v_options[i];
                }
            }));
            policy.lock(ui, &["Margin"], |ui| slider(ui, tr("Gap from the edge"), "", &mut menu.cfg.margin, 0.0..=80.0, pal)); // i18n-ignore: policy name
            policy.lock(ui, &["Width"], |ui| slider(ui, tr("Width"), "", &mut menu.cfg.width, 420.0..=1200.0, pal)); // i18n-ignore: policy name
            policy.lock(ui, &["Height"], |ui| slider(ui, tr("Height"), "", &mut menu.cfg.height, 420.0..=1100.0, pal)); // i18n-ignore: policy name
        });
    });
}

fn general(menu: &mut Menu, ui: &mut Ui, pal: Pal) {
    page_header(ui, tr("General"), tr("How Startline fits into Windows."), pal);
    let policy = menu.policy.clone();
    two_columns(menu, ui, |menu, ui| {
        heading(ui, tr("With Windows"), pal);
        card(ui, pal, |ui| {
            let mut autostart = settings::autostart_enabled();
            if policy.lock(ui, &["StartWithWindows"], |ui| switch_row(ui, tr("Start with Windows"), tr("Keeps your Start menu working after a restart."), &mut autostart, pal)) {
                settings::set_autostart(autostart);
            }
            policy.lock(ui, &["TakeOverStartKey"], |ui| switch_row(ui, tr("Start key and button open this menu"), tr("Shift + Start still opens the Windows menu."), &mut menu.cfg.take_over_start, pal));
            policy.lock(ui, &["TakeOverSearchKey"], |ui| switch_row(ui, tr("Start + S opens this menu too"), tr("Your other shortcuts keep working."), &mut menu.cfg.take_over_search, pal));
        });
        heading(ui, tr("If something's off"), pal);
        ui.horizontal_wrapped(|ui| {
            if policy.allows_do(Do::RealStart) && pill_button(ui, tr("Open the Windows Start menu"), pal, false).clicked() {
                menu.intents.push(Intent::Run(Do::RealStart));
            }
            if policy.allows("Command_SettingsFolder") && pill_button(ui, tr("Open my settings folder"), pal, false).clicked() {
                platform::open_async("", data_dir().to_string_lossy().into_owned(), String::new());
            }
            let armed = menu.reset_armed.is_some_and(|at| at.elapsed() < Duration::from_secs(5));
            let label = if armed { tr("Click again to reset everything") } else { tr("Reset all settings") };
            if policy.allows("Command_ResetSettings") && pill_button(ui, label, pal, armed).clicked() {
                if armed {
                    let usage = std::mem::take(&mut menu.cfg.usage);
                    menu.cfg = Config { usage, seen_welcome: true, autostart_applied: true, ..Default::default() };
                    menu.reset_armed = None;
                } else {
                    menu.reset_armed = Some(Instant::now());
                }
            }
            if policy.allows_do(Do::Quit) && pill_button(ui, tr("Quit Startline"), pal, false).clicked() {
                menu.intents.push(Intent::Run(Do::Quit));
            }
        });
    }, |_menu, ui| {
        heading(ui, tr("Handy keys"), pal);
        card(ui, pal, |ui| {
            for (keys, what) in [
                (tx("Type"), tx("Search right away")),
                (tx("Enter"), tx("Open, or apply a setting")),
                (tx("Down, then left and right"), tx("Change a selected setting")),
                (tx("Ctrl + Enter"), tx("Run as administrator")),
                (tx("Shift + Enter"), tx("Show in its folder")),
                (".pdf", tx("Only one kind of file")),
                (tx("Esc"), tx("Clear, go back, then close")),
                (tx("Shift + Start"), tx("The Windows Start menu")),
            ] {
                ui.horizontal(|ui| {
                    ui.set_min_height(24.0);
                    ui.label(RichText::new(tr(what)).size(13.0).color(pal.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(tr(keys)).size(12.5).color(pal.sub));
                    });
                });
            }
        });
    });
}

fn about(menu: &mut Menu, ui: &mut Ui, pal: Pal) {
    page_header(ui, tr("About & updates"), tr("Which version this is, and how it stays up to date."), pal);
    two_columns(menu, ui, updates_card, |_menu, ui| {
        heading(ui, tr("Startline"), pal);
        card(ui, pal, |ui| {
            ui.horizontal(|ui| {
                let logo = ui.ctx().data_mut(|d| d.get_temp::<TextureHandle>(Id::unique("startline-logo"))).unwrap_or_else(|| {
                    let image = ColorImage::from_rgba_unmultiplied([128, 128], &crate::app_icon_pixels(128));
                    let texture = ui.ctx().load_texture("startline-logo", image, TextureOptions::LINEAR);
                    ui.ctx().data_mut(|d| d.insert_temp(Id::unique("startline-logo"), texture.clone()));
                    texture
                });
                let shown = ui.add(Image::new(&logo).fit_to_exact_size(vec2(56.0, 56.0))).rect;
                // The tile is the dark themes' own panel color, so it needs an edge.
                ui.painter().rect_stroke(shown, 12, Stroke::new(1.0, pal.line), StrokeKind::Inside);
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(RichText::new("Startline").font(semibold(18.0)).color(pal.text)); // i18n-ignore: the name
                    ui.label(RichText::new(trf!("Version {version}", version = crate::update::CURRENT)).size(13.0).color(pal.text));
                    ui.label(RichText::new(tr("A Start menu with search that answers as you type. Free software under the GPL-3.0 licence.")).size(12.0).color(pal.sub));
                });
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                for (label, url) in [
                    (tr("Project"), crate::update::PROJECT.to_string()),
                    (tr("What's new"), crate::update::RELEASES.to_string()),
                    (tr("Report a problem"), format!("{}/issues", crate::update::PROJECT)),
                    (tr("Licence"), format!("{}/blob/main/LICENSE", crate::update::PROJECT)),
                ] {
                    if pill_button(ui, label, pal, false).on_hover_text(&url).clicked() {
                        platform::open_async("", url, String::new());
                    }
                }
            });
        });
    });
}

/// Whether there's a newer version, and whether to install it without asking.
fn updates_card(menu: &mut Menu, ui: &mut Ui) {
    use crate::update::{self, Install, Status};
    let pal = menu.pal();
    let policy = menu.policy.clone();
    heading(ui, tr("Updates"), pal);
    let Some(updater) = &menu.updater else {
        card(ui, pal, |ui| note(ui, tr("This copy doesn't look for updates."), pal));
        return;
    };
    let (status, install) = (updater.status(), updater.install.clone());
    let machine = matches!(install, Install::Machine { .. });
    let own_auto = menu.cfg.auto_update;
    let mut check = false;
    let mut get = false;
    let mut switched = None;
    card(ui, pal, |ui| {
        if !update::allowed(&policy) {
            note(ui, tr("Your organization installs new versions of Startline, so it doesn't look for updates itself."), pal);
            return;
        }
        note(
            ui,
            match &install {
                Install::PerUser { .. } => tr("Installed for this account. Updates install by themselves while the menu is closed, and Startline starts again straight away."),
                Install::Machine { .. } => tr("Installed for this PC. The Startline Updater task installs updates within the hour, and starts Startline again for everyone using it."),
                Install::Portable(_) => tr("A single program file. Updates replace it, and Startline starts again with the new one."),
                Install::Unmanaged => tr("This copy can't update itself. New versions are on the releases page."),
            },
            pal,
        );
        // The whole PC shares one switch, which the task reads: show what it's really set to.
        let mut auto = if machine { policy.fixed("/auto_update").and_then(serde_json::Value::as_bool).or_else(update::machine_switch).unwrap_or(true) } else { own_auto };
        if policy.lock(ui, &["AutomaticUpdates"], |ui| switch_row(ui, tr("Install updates automatically"), tr("Looks at start and every 6 hours."), &mut auto, pal)) {
            switched = Some(auto);
        }
        let text = match &status {
            Status::Idle => tr("Not checked yet.").to_string(),
            Status::Checking => tr("Checking for updates…").to_string(),
            Status::UpToDate => tr("Startline is up to date.").to_string(),
            Status::Available(v) if machine => trf!("Version {v} is available. The update task installs it once automatic updates are on.", v = v),
            Status::Available(v) => trf!("Version {v} is available.", v = v),
            Status::Downloading(v) => trf!("Downloading version {v}…", v = v),
            Status::Ready(v) if own_auto => trf!("Version {v} is ready. It installs the next time the menu and this window are closed.", v = v),
            Status::Ready(v) => trf!("Version {v} is downloaded.", v = v),
            Status::InstallsByTask(v) => trf!("Version {v} is available. The update task installs it within the hour.", v = v),
            Status::Installing(v) => trf!("Installing version {v}. Startline starts again in a moment.", v = v),
            Status::Failed(e) => e.clone(),
        };
        let color = if matches!(status, Status::Failed(_)) { Color32::from_rgb(222, 120, 96) } else { pal.text };
        ui.label(RichText::new(text).size(13.0).color(color));
        let busy = matches!(status, Status::Checking | Status::Downloading(_) | Status::Installing(_));
        let can_install = !machine && install != Install::Unmanaged;
        ui.horizontal_wrapped(|ui| {
            check = ui.add_enabled_ui(!busy, |ui| pill_button(ui, tr("Check for updates"), pal, false)).inner.clicked();
            match &status {
                Status::Available(_) if can_install => get = pill_button(ui, tr("Install and restart"), pal, true).clicked(),
                Status::Ready(_) => get = pill_button(ui, tr("Restart now to update"), pal, true).clicked(),
                _ => {}
            }
        });
    });
    if check {
        updater.check_now(own_auto);
    }
    if get && matches!(status, Status::Available(_)) {
        updater.download_now();
    }
    match switched {
        Some(on) if machine => {
            if let Err(e) = update::set_machine_switch(on) {
                updater.fail(e);
            }
        }
        Some(on) => menu.cfg.auto_update = on,
        None => {}
    }
    // Installs as soon as it's downloaded, as the person asked for it.
    if get {
        menu.update_requested = true;
    }
}
