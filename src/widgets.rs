//! Colors, fonts and the small hand-drawn controls used across the menu.

use crate::styles::{Experience, Look};
use egui::text::{LayoutJob, TextFormat};
use egui::*;
use std::sync::Arc;

#[derive(Clone, Copy)]
pub struct Pal {
    pub bg: Color32,
    pub surface: Color32,
    pub hover: Color32,
    pub text: Color32,
    pub sub: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub line: Color32,
    pub selected: Color32,
    pub style: Experience,
    /// How round small things are: rows, buttons, cards.
    pub radius: u8,
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

fn luminance(c: [u8; 3]) -> f32 {
    (0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32) / 255.0
}

pub fn palette(look: &Look) -> Pal {
    let theme = &look.theme;
    let style = look.experience;
    let (bg, surface, text, accent) = (rgb(theme.bg), rgb(theme.surface), rgb(theme.text), rgb(theme.accent));
    let selected = match style {
        Experience::Startline | Experience::Windows11 | Experience::Windows10 => mix(surface, text, 0.10),
        Experience::Windows7 => mix(Color32::WHITE, accent, 0.20),
        Experience::WindowsXp => accent,
    };
    let radius = match style {
        Experience::Startline => 6,
        Experience::Windows11 => 4,
        Experience::Windows10 => 0,
        Experience::Windows7 | Experience::WindowsXp => 3,
    };
    Pal {
        bg,
        surface,
        hover: mix(surface, text, 0.08),
        text,
        sub: rgb(theme.subtext),
        accent,
        on_accent: if luminance(theme.accent) > 0.55 { Color32::from_rgb(10, 12, 18) } else { Color32::WHITE },
        line: mix(bg, text, 0.13),
        selected,
        style,
        radius,
    }
}

/// The highlight behind a chosen row, the way each look draws it.
pub fn paint_selection(ui: &Ui, rect: Rect, pal: Pal) {
    ui.painter().rect_filled(rect, pal.radius, pal.selected);
    if pal.style == Experience::Windows7 {
        ui.painter().rect_stroke(rect, pal.radius, Stroke::new(1.0, mix(Color32::WHITE, pal.accent, 0.55)), StrokeKind::Inside);
    }
}

/// A fill that fades from top to bottom, for glass and the blue bars of the older looks.
pub fn vertical_gradient(ui: &Ui, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 2, 3);
    ui.painter().add(Shape::mesh(mesh));
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("icons".into()))
}

fn one_line(max_width: f32) -> text::TextWrapping {
    text::TextWrapping {
        max_width: max_width.max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    }
}

pub fn line_galley(ui: &Ui, text: &str, font: FontId, color: Color32, max_width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = one_line(max_width);
    ui.painter().layout_job(job)
}

/// A folder path too long for its line loses its middle: the last folders are what tell
/// similar results apart.
pub fn path_galley(ui: &Ui, path: &str, font: FontId, color: Color32, max_width: f32) -> Arc<Galley> {
    let whole = ui.painter().layout_no_wrap(path.to_string(), font.clone(), color);
    if whole.size().x <= max_width {
        return whole;
    }
    let chars: Vec<char> = path.chars().collect();
    let room = ((max_width * chars.len() as f32 / whole.size().x) as usize).saturating_sub(2);
    let tail = (room * 2 / 3).min(chars.len());
    let head = (room - tail).min(chars.len() - tail);
    let short: String = chars[..head].iter().chain(['…'].iter()).chain(chars[chars.len() - tail..].iter()).collect();
    line_galley(ui, &short, font, color, max_width)
}

/// The name with the typed words picked out in the accent color.
pub fn highlighted_galley(ui: &Ui, text: &str, tokens: &[String], font: FontId, color: Color32, accent: Color32, max_width: f32) -> Arc<Galley> {
    let lowered = crate::matching::lower(text);
    let mut job = LayoutJob::default();
    job.wrap = one_line(max_width);
    let plain = TextFormat { font_id: font.clone(), color, ..Default::default() };
    let strong = TextFormat { font_id: semibold(font.size), color: accent, ..Default::default() };
    // Lowercasing can change byte positions in rare alphabets; then there is nothing safe to pick out.
    let mut marks: Vec<(usize, usize)> = Vec::new();
    if lowered.len() == text.len() {
        for token in tokens.iter().filter(|t| !t.starts_with("ext:")) {
            if let Some(pos) = lowered.find(token.as_str()) {
                marks.push((pos, pos + token.len()));
            }
        }
        marks.sort_unstable();
    }
    let mut at = 0;
    for (start, end) in marks {
        if start < at || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            continue;
        }
        job.append(&text[at..start], 0.0, plain.clone());
        job.append(&text[start..end], 0.0, strong.clone());
        at = end;
    }
    job.append(&text[at..], 0.0, plain);
    ui.painter().layout_job(job)
}

pub fn section_title(ui: &mut Ui, text: &str, pal: Pal) {
    ui.add_space(4.0);
    ui.label(RichText::new(text).font(semibold(13.0)).color(pal.sub));
    ui.add_space(3.0);
}

pub fn pill_button(ui: &mut Ui, text: &str, pal: Pal, primary: bool) -> Response {
    let font = FontId::proportional(13.0);
    let classic = pal.style.is_classic();
    let (fill, color) = if primary && !classic {
        (pal.accent, pal.on_accent)
    } else if classic {
        (Color32::from_rgb(244, 244, 240), Color32::BLACK)
    } else {
        (pal.surface, pal.text)
    };
    let galley = line_galley(ui, text, font, color, 400.0);
    let size = galley.size() + vec2(24.0, 12.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if response.hovered() { mix(fill, pal.text, 0.12) } else { fill };
    if classic {
        vertical_gradient(ui, rect, Color32::WHITE, fill);
        let border = if primary { pal.accent } else { Color32::from_rgb(112, 112, 112) };
        ui.painter().rect_stroke(rect, pal.radius, Stroke::new(1.0, border), StrokeKind::Inside);
    } else {
        ui.painter().rect_filled(rect, pal.radius, fill);
        if !primary && pal.style != Experience::Startline {
            ui.painter().rect_stroke(rect, pal.radius, Stroke::new(1.0, pal.line), StrokeKind::Inside);
        }
    }
    ui.painter().galley(rect.min + vec2(12.0, 6.0 - ink_nudge(13.0)), galley, color);
    response
}

pub fn icon_button(ui: &mut Ui, rect: Rect, id: Id, glyph: char, tip: &str, active: bool, pal: Pal) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    if active {
        ui.painter().rect_filled(rect, pal.radius, pal.selected);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, pal.radius, pal.hover);
    }
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, glyph, icon_font(16.0), pal.text);
    response.on_hover_text(tip)
}

pub fn draw_switch(ui: &Ui, rect: Rect, on: bool, pal: Pal) {
    // Those versions of Windows had check boxes, not switches.
    if pal.style.is_classic() {
        let box_rect = Rect::from_center_size(pos2(rect.right() - 9.0, rect.center().y), Vec2::splat(15.0));
        ui.painter().rect_filled(box_rect, 1, Color32::WHITE);
        ui.painter().rect_stroke(box_rect, 1, Stroke::new(1.0, Color32::from_rgb(28, 81, 128)), StrokeKind::Inside);
        if on {
            let mark = [box_rect.left_center() + vec2(3.0, 0.0), box_rect.center_bottom() + vec2(-1.0, -4.0), box_rect.right_top() + vec2(-3.0, 4.0)];
            ui.painter().line(mark.to_vec(), Stroke::new(2.0, Color32::from_rgb(33, 161, 33)));
        }
        return;
    }
    if pal.style == Experience::Windows10 {
        let radius = rect.height() / 2.0;
        let stroke = if on { pal.accent } else { pal.text };
        if on {
            ui.painter().rect_filled(rect, radius, pal.accent);
        }
        ui.painter().rect_stroke(rect, radius, Stroke::new(2.0, stroke), StrokeKind::Inside);
        let x = if on { rect.right() - radius } else { rect.left() + radius };
        ui.painter().circle_filled(pos2(x, rect.center().y), radius - 6.0, if on { pal.on_accent } else { pal.text });
        return;
    }
    let radius = rect.height() / 2.0;
    let fill = if on { pal.accent } else { mix(pal.surface, pal.text, 0.22) };
    ui.painter().rect_filled(rect, radius, fill);
    let x = if on { rect.right() - radius } else { rect.left() + radius };
    let knob = if on { pal.on_accent } else { pal.text };
    ui.painter().circle_filled(pos2(x, rect.center().y), radius - 4.0, knob);
}

pub fn switch(ui: &mut Ui, value: &mut bool, pal: Pal) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(40.0, 22.0), Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    draw_switch(ui, rect, *value, pal);
    response
}

#[allow(clippy::too_many_arguments)]
pub fn draw_slider(ui: &Ui, area: Rect, id: Id, value: f32, min: f32, max: f32, step: f32, pal: Pal) -> Option<f32> {
    let track = Rect::from_center_size(area.center(), vec2(area.width() - 16.0, 6.0));
    let response = ui.interact(area, id, Sense::click_and_drag());
    let fraction = ((value - min) / (max - min)).clamp(0.0, 1.0);
    let knob_x = track.left() + track.width() * fraction;
    ui.painter().rect_filled(track, 3, mix(pal.surface, pal.text, 0.22));
    let filled = Rect::from_min_max(track.min, pos2(knob_x, track.bottom()));
    ui.painter().rect_filled(filled, 3, pal.accent);
    if matches!(pal.style, Experience::Windows10 | Experience::Windows7 | Experience::WindowsXp) {
        let thumb = Rect::from_center_size(pos2(knob_x, track.center().y), vec2(9.0, 20.0));
        ui.painter().rect_filled(thumb, pal.radius.min(2), pal.accent);
    } else {
        let grow = if response.hovered() || response.dragged() { 8.0 } else { 7.0 };
        ui.painter().circle_filled(pos2(knob_x, track.center().y), grow, pal.accent);
        ui.painter().circle_filled(pos2(knob_x, track.center().y), grow - 3.5, pal.on_accent);
    }
    if response.dragged() || response.clicked() {
        let pointer = response.interact_pointer_pos()?;
        let t = ((pointer.x - track.left()) / track.width()).clamp(0.0, 1.0);
        let stepped = ((min + (max - min) * t) / step).round() * step;
        if (stepped - value).abs() >= step / 2.0 {
            return Some(stepped);
        }
    }
    None
}

pub fn segmented_at(ui: &Ui, area: Rect, id: Id, options: &[&str], current: usize, pal: Pal) -> Option<usize> {
    let mut picked = None;
    ui.painter().rect_filled(area, pal.radius, pal.surface);
    if pal.style != Experience::Startline {
        ui.painter().rect_stroke(area, pal.radius, Stroke::new(1.0, pal.line), StrokeKind::Inside);
    }
    let width = area.width() / options.len() as f32;
    for (i, option) in options.iter().enumerate() {
        let cell = Rect::from_min_size(area.min + vec2(width * i as f32, 0.0), vec2(width, area.height()));
        let response = ui.interact(cell, id.with(i), Sense::click());
        let active = i == current;
        if active {
            ui.painter().rect_filled(cell.shrink(2.0), pal.radius, pal.accent);
        } else if response.hovered() {
            ui.painter().rect_filled(cell.shrink(2.0), pal.radius, pal.hover);
        }
        let color = if active { pal.on_accent } else { pal.text };
        ui.painter().text(cell.center() - vec2(0.0, ink_nudge(12.5)), Align2::CENTER_CENTER, *option, FontId::proportional(12.5), color);
        if response.clicked() {
            picked = Some(i);
        }
    }
    picked
}

/// Side by side when there's room, one above the other when not.
pub fn two_columns<T>(state: &mut T, ui: &mut Ui, left: impl FnOnce(&mut T, &mut Ui), right: impl FnOnce(&mut T, &mut Ui)) {
    let room = ui.available_width();
    if room < 780.0 {
        left(state, ui);
        ui.add_space(6.0);
        right(state, ui);
        return;
    }
    let gap = 24.0;
    let width = (room - gap) / 2.0;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.set_width(width);
            left(state, ui);
        });
        ui.add_space(gap);
        ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.set_width(width);
            right(state, ui);
        });
    });
}

/// Where the icons in a list line up, measured from the list's left edge. The search bar's
/// magnifier, every row's icon and your picture all share this line.
pub const ICON_COLUMN: f32 = 21.0;
/// Where the words beside those icons start.
pub const TEXT_COLUMN: f32 = 44.0;

/// How far to lift text so its letters look centered. A line of text has more room above the
/// letters than below them, so centering the line leaves the letters sitting low.
pub fn ink_nudge(font_size: f32) -> f32 {
    font_size * 0.065
}

/// How wide a chip with this text will be, for fitting them into a row.
pub fn chip_width(ui: &Ui, text: &str, count: Option<u32>) -> f32 {
    let label = line_galley(ui, text, FontId::proportional(12.5), Color32::WHITE, 220.0).size().x;
    let number = count.map_or(0.0, |n| line_galley(ui, &thousands(n as usize), FontId::proportional(11.0), Color32::WHITE, 60.0).size().x + 6.0);
    label + number + 20.0
}

/// A small rounded button with an optional count, used for filters and rule tags.
pub fn chip(ui: &mut Ui, text: &str, count: Option<u32>, active: bool, pal: Pal) -> Response {
    let font = FontId::proportional(12.5);
    let color = if active { pal.on_accent } else { pal.text };
    let galley = line_galley(ui, text, font.clone(), color, 220.0);
    let count_galley = count.map(|n| line_galley(ui, &thousands(n as usize), FontId::proportional(11.0), if active { pal.on_accent } else { pal.sub }, 60.0));
    let extra = count_galley.as_ref().map_or(0.0, |g| g.size().x + 6.0);
    let size = vec2(galley.size().x + extra + 20.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let round = match pal.style {
        Experience::Windows10 => 0,
        Experience::Startline => pal.radius,
        _ => 13,
    };
    let fill = if active {
        pal.accent
    } else if response.hovered() {
        pal.hover
    } else {
        pal.surface
    };
    ui.painter().rect_filled(rect, round, fill);
    if !active {
        ui.painter().rect_stroke(rect, round, Stroke::new(1.0, pal.line), StrokeKind::Inside);
    }
    let y = rect.center().y - galley.size().y / 2.0 - ink_nudge(12.5);
    let text_width = galley.size().x;
    ui.painter().galley(pos2(rect.left() + 10.0, y), galley, color);
    if let Some(number) = count_galley {
        let y = rect.center().y - number.size().y / 2.0 - ink_nudge(11.0);
        ui.painter().galley(pos2(rect.left() + 16.0 + text_width, y), number, pal.sub);
    }
    response
}

/// A list you reorder by dragging a row's grip. `draw` fills in each row beside the grip.
pub fn drag_rows<T>(ui: &mut Ui, id: Id, list: &mut [T], height: f32, pal: Pal, mut draw: impl FnMut(&mut Ui, &mut T)) {
    let mut dragging: Option<usize> = ui.data(|d| d.get_temp(id));
    let pointer_down = ui.input(|i| i.pointer.any_down());
    if !pointer_down {
        dragging = None;
    }
    let width = ui.available_width();
    let mut rects = Vec::with_capacity(list.len());
    for (index, item) in list.iter_mut().enumerate() {
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        rects.push(rect);
        if dragging == Some(index) {
            ui.painter().rect_filled(rect, pal.radius, pal.selected);
        }
        let grip = Rect::from_min_size(rect.min, vec2(28.0, height));
        let response = ui.interact(grip, id.with(index), Sense::drag()).on_hover_cursor(CursorIcon::Grab);
        let lit = response.hovered() || dragging == Some(index);
        ui.painter().text(grip.center(), Align2::CENTER_CENTER, '\u{E76F}', icon_font(14.0), if lit { pal.text } else { pal.sub });
        if response.drag_started() {
            dragging = Some(index);
        }
        let body = Rect::from_min_max(pos2(grip.right() + 4.0, rect.top()), rect.max);
        let mut row = ui.new_child(UiBuilder::new().max_rect(body).layout(Layout::left_to_right(Align::Center)));
        draw(&mut row, item);
    }
    if let (Some(from), Some(pointer)) = (dragging, ui.input(|i| i.pointer.interact_pos())) {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        let to = rects.iter().position(|r| pointer.y < r.bottom()).unwrap_or(rects.len().saturating_sub(1));
        if to != from && from < list.len() {
            if to < from {
                list[to..=from].rotate_right(1);
            } else {
                list[from..=to].rotate_left(1);
            }
            dragging = Some(to);
        }
    }
    ui.data_mut(|d| match dragging {
        Some(index) => {
            d.insert_temp(id, index);
        }
        None => d.remove::<usize>(id),
    });
}

/// A box that can be ticked, partly ticked or empty.
pub fn draw_tick(ui: &Ui, rect: Rect, state: crate::scope::Tick, pal: Pal) {
    use crate::scope::Tick;
    let square = Rect::from_center_size(rect.center(), Vec2::splat(16.0));
    let round = pal.radius.min(3);
    match state {
        Tick::Off => {
            ui.painter().rect_stroke(square, round, Stroke::new(1.5, pal.sub), StrokeKind::Inside);
        }
        Tick::On => {
            ui.painter().rect_filled(square, round, pal.accent);
            let mark = vec![square.left_center() + vec2(3.5, 0.0), square.center_bottom() + vec2(-1.5, -4.5), square.right_top() + vec2(-3.5, 4.5)];
            ui.painter().line(mark, Stroke::new(2.0, pal.on_accent));
        }
        Tick::Some => {
            ui.painter().rect_filled(square, round, pal.accent);
            let bar = Rect::from_center_size(square.center(), vec2(8.0, 2.0));
            ui.painter().rect_filled(bar, 0, pal.on_accent);
        }
    }
}

/// Choices shown as side-by-side buttons, for the Customize page.
pub fn segmented(ui: &mut Ui, id: &str, options: &[&str], current: usize, width: f32, pal: Pal) -> Option<usize> {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 30.0), Sense::hover());
    segmented_at(ui, rect, Id::unique(id), options, current, pal)
}

pub fn relative_time(seconds: u32) -> String {
    match seconds {
        0..60 => "Just now".into(),
        60..3600 => format!("{} min ago", seconds / 60),
        3600..86_400 => format!("{} h ago", seconds / 3600),
        86_400..172_800 => "Yesterday".into(),
        _ => format!("{} days ago", seconds / 86_400),
    }
}

pub fn thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn install_fonts(ctx: &Context, style: Experience) {
    let mut fonts = FontDefinitions::default();
    let dir = std::path::Path::new("C:\\Windows\\Fonts");
    let mut add = |name: &str, file: &str| -> bool {
        match std::fs::read(dir.join(file)) {
            Ok(bytes) => {
                fonts.font_data.insert(name.to_string(), Arc::new(FontData::from_owned(bytes)));
                true
            }
            Err(_) => false,
        }
    };
    // Windows XP's menus were set in Tahoma, which Windows still ships.
    let xp = style == Experience::WindowsXp;
    let has_ui = add("segoe", if xp { "tahoma.ttf" } else { "segoeui.ttf" });
    let has_bold = add("segoe-semibold", if xp { "tahomabd.ttf" } else { "seguisb.ttf" });
    let has_icons = add("segoe-icons", "segmdl2.ttf");
    // Names in other alphabets still need to show up in results.
    let has_cjk = add("msyh", "msyh.ttc");
    let has_korean = add("malgun", "malgun.ttf");

    let mut fallback = Vec::new();
    if has_cjk {
        fallback.push("msyh".to_string());
    }
    if has_korean {
        fallback.push("malgun".to_string());
    }

    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    if has_ui {
        proportional.insert(0, "segoe".into());
    }
    proportional.extend(fallback.iter().cloned());
    let base = proportional.clone();
    let mut bold = base.clone();
    if has_bold {
        bold.insert(0, "segoe-semibold".into());
    }
    fonts.families.insert(FontFamily::Name("semibold".into()), bold);
    let mut glyphs = Vec::new();
    if has_icons {
        glyphs.push("segoe-icons".to_string());
    }
    glyphs.extend(base);
    fonts.families.insert(FontFamily::Name("icons".into()), glyphs);
    ctx.set_fonts(fonts);
}

pub fn apply_style(ctx: &Context, look: &Look) {
    let pal = palette(look);
    let dark = luminance(look.theme.bg) < 0.5;
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    let mut visuals = if dark { Visuals::dark() } else { Visuals::light() };
    visuals.override_text_color = Some(pal.text);
    visuals.panel_fill = pal.bg;
    visuals.window_fill = pal.surface;
    // Text fields sit on panels, so they take the darker window color to read as fields.
    visuals.extreme_bg_color = if pal.style.is_classic() { Color32::WHITE } else { pal.bg };
    visuals.faint_bg_color = pal.surface;
    visuals.window_stroke = Stroke::new(1.0, pal.line);
    visuals.window_corner_radius = CornerRadius::same(pal.radius + 2);
    visuals.menu_corner_radius = CornerRadius::same(pal.radius + 2);
    visuals.selection.bg_fill = pal.accent.gamma_multiply(0.45);
    visuals.selection.stroke = Stroke::new(1.0, pal.accent);
    visuals.hyperlink_color = pal.accent;
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(pal.radius);
        widget.fg_stroke.color = pal.text;
    }
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, pal.line);
    visuals.widgets.inactive.weak_bg_fill = pal.surface;
    visuals.widgets.inactive.bg_fill = mix(pal.surface, pal.text, 0.16);
    visuals.widgets.hovered.weak_bg_fill = pal.hover;
    visuals.widgets.hovered.bg_fill = mix(pal.surface, pal.text, 0.24);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, pal.line);
    visuals.widgets.active.weak_bg_fill = pal.selected;
    visuals.widgets.active.bg_fill = pal.accent;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, pal.accent);
    visuals.widgets.open.weak_bg_fill = pal.hover;
    ctx.set_visuals(visuals);
    ctx.set_zoom_factor(look.ui_scale.clamp(0.7, 1.6));
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = vec2(8.0, 5.0);
        style.spacing.button_padding = vec2(10.0, 5.0);
        style.spacing.interact_size.y = 26.0;
        style.spacing.slider_width = 200.0;
        style.spacing.scroll = style::ScrollStyle::floating();
        style.text_styles.insert(TextStyle::Body, FontId::proportional(13.5));
        style.text_styles.insert(TextStyle::Button, FontId::proportional(13.5));
    });
}

pub fn save_png(path: &std::path::Path, image: &ColorImage) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let [width, height] = image.size;
    let backdrop = [34.0f32, 38.0, 48.0];
    let mut bytes = Vec::with_capacity(width * height * 3);
    for pixel in &image.pixels {
        // The window is see-through at the rounded corners; lay it over a neutral desktop color.
        let rest = 1.0 - pixel.a() as f32 / 255.0;
        bytes.push((pixel.r() as f32 + backdrop[0] * rest) as u8);
        bytes.push((pixel.g() as f32 + backdrop[1] * rest) as u8);
        bytes.push((pixel.b() as f32 + backdrop[2] * rest) as u8);
    }
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&bytes)?;
    Ok(())
}
