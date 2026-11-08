//! Drawing the menu's screens: search bar, home sections, results and footer.

use crate::actions::Do;
use crate::config::{Category, Pin, SectionKind, Target};
use crate::customize::Tab;
use crate::icons;
use crate::intent::{self, Want};
use crate::platform;
use crate::popup::{self, Entry};
use crate::search::{FILE_KINDS, Icon, Row, RowKind};
use crate::settings::{Control, QuickId, Value};
use crate::styles::Experience;
use crate::ui::{Intent, Menu, NavEntry, NavKind, Page};
use crate::widgets::*;
use egui::*;
use std::sync::atomic::Ordering;
use std::time::Duration;

impl Menu {
    pub(crate) fn draw(&mut self, ui: &mut Ui) {
        let full = ui.max_rect();
        let pal = self.pal();
        let look = self.look;
        let in_settings = matches!(self.page, Page::Customize(_));
        let style = look.experience;
        let opacity = if in_settings && style != Experience::Startline { 1.0 } else { look.opacity };
        let alpha = (opacity.clamp(0.25, 1.0) * 255.0) as u8;
        let fill = Color32::from_rgba_unmultiplied(pal.bg.r(), pal.bg.g(), pal.bg.b(), alpha);
        // Exactly the window's own corner, so no second shape can show around the paint.
        let round = look.corner_points(ui.ctx().pixels_per_point()).round() as u8;
        let radius = if look.round_bottom { CornerRadius::same(round) } else { CornerRadius { nw: round, ne: round, sw: 0, se: 0 } };
        // Windows 7's menu is glass all the way to the edge and paints its own.
        if in_settings || style != Experience::Windows7 {
            ui.painter().rect_filled(full, radius, fill);
        }
        if look.native_corner.is_none() {
            ui.painter().rect_stroke(full.shrink(0.5), radius, Stroke::new(1.0, pal.line), StrokeKind::Inside);
        }

        if let Page::Customize(tab) = self.page {
            let mut child = ui.new_child(UiBuilder::new().max_rect(full));
            crate::customize::draw(self, &mut child, tab);
            return;
        }
        match style {
            Experience::Windows10 => self.draw_win10(ui, full, pal),
            Experience::Windows7 => self.draw_win7(ui, full, pal),
            Experience::WindowsXp => self.draw_xp(ui, full, pal),
            Experience::Windows11 | Experience::Startline => self.draw_modern(ui, full, pal),
        }
    }

    /// The Windows 11 and Startline menus: a search bar, a scrolling middle and a footer.
    fn draw_modern(&mut self, ui: &mut Ui, full: Rect, pal: Pal) {
        let content = full.shrink(14.0);
        let search_h = if pal.style == Experience::Startline { 40.0 } else { 46.0 };
        let footer_h = 40.0;
        let gap = 10.0;
        let footer = Rect::from_min_max(pos2(content.left(), content.bottom() - footer_h), content.right_bottom());
        let search_low = pal.style == Experience::Startline && self.cfg.search_at_bottom;
        if pal.style == Experience::Windows11 {
            let round = self.look.corner_points(ui.ctx().pixels_per_point()).round() as u8;
            let band = Rect::from_min_max(pos2(full.left(), footer.top() - 4.0), full.max);
            let tint = if self.system_look_dark() { Color32::from_rgba_unmultiplied(0, 0, 0, 46) } else { Color32::from_rgba_unmultiplied(0, 0, 0, 12) };
            ui.painter().rect_filled(band, CornerRadius { nw: 0, ne: 0, sw: round, se: round }, tint);
        }
        // The file type buttons sit against the search bar, on the side the results are.
        let chips_h = self.chip_row_height();
        let chip_room = if chips_h > 0.0 { chips_h + 6.0 } else { 0.0 };
        let (search_rect, chip_rect, body) = if search_low {
            let search = Rect::from_min_max(pos2(content.left(), footer.top() - gap - search_h), pos2(content.right(), footer.top() - gap));
            let chips = Rect::from_min_max(pos2(content.left(), search.top() - chip_room), pos2(content.right(), search.top() - 6.0));
            (search, chips, Rect::from_min_max(content.left_top(), pos2(content.right(), search.top() - chip_room - gap)))
        } else {
            let search = Rect::from_min_size(content.left_top(), vec2(content.width(), search_h));
            let chips = Rect::from_min_max(pos2(content.left(), search.bottom() + 6.0), pos2(content.right(), search.bottom() + chip_room));
            (search, chips, Rect::from_min_max(pos2(content.left(), search.bottom() + chip_room + gap), pos2(content.right(), footer.top() - gap)))
        };

        self.draw_search_bar(ui, search_rect, pal);
        self.draw_filter_chips(ui, chip_rect, pal);
        self.scroll_in(ui, body, "body", |menu, ui| {
            if !menu.query.trim().is_empty() {
                menu.draw_results(ui, pal);
            } else {
                match menu.page {
                    Page::AllApps => menu.draw_all_apps_page(ui, pal),
                    _ => menu.draw_home(ui, pal),
                }
            }
        });
        self.draw_footer(ui, footer, pal);
    }

    fn system_look_dark(&self) -> bool {
        (self.look.theme.bg[0] as u32) < 128
    }

    pub(crate) fn draw_search_bar(&mut self, ui: &mut Ui, rect: Rect, pal: Pal) {
        let style = pal.style;
        let (radius, fill, border) = match style {
            Experience::Startline => (CornerRadius::same(pal.radius), pal.surface, pal.surface),
            Experience::Windows11 => (CornerRadius::same((rect.height() / 2.0) as u8), pal.surface, pal.line),
            Experience::Windows10 => (CornerRadius::ZERO, pal.surface, pal.line),
            Experience::Windows7 => (CornerRadius::same(3), Color32::WHITE, Color32::from_rgb(142, 164, 186)),
            Experience::WindowsXp => (CornerRadius::same(2), Color32::WHITE, Color32::from_rgb(127, 157, 185)),
        };
        ui.painter().rect_filled(rect, radius, fill);
        ui.painter().rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);
        // The older menus keep the magnifier at the far end of the box.
        let glass_at_end = style.is_classic();
        let glass_x = if glass_at_end { rect.right() - 16.0 } else { rect.left() + ICON_COLUMN };
        let text_left = if glass_at_end { rect.left() + 8.0 } else { rect.left() + TEXT_COLUMN };
        let font_size = if glass_at_end { 13.0 } else { 15.0 };
        ui.painter().text(pos2(glass_x, rect.center().y), Align2::CENTER_CENTER, '\u{E721}', icon_font(if glass_at_end { 13.0 } else { 16.0 }), pal.sub);
        let timing = if glass_at_end || style != Experience::Startline { String::new() } else { self.timing_label() };
        let timing_w = if timing.is_empty() {
            0.0
        } else {
            let galley = line_galley(ui, &timing, FontId::proportional(12.0), pal.sub, 200.0);
            let width = galley.size().x;
            ui.painter().galley(pos2(rect.right() - width - 14.0, rect.center().y - galley.size().y / 2.0 - ink_nudge(12.0)), galley, pal.sub);
            width + 20.0
        };
        let end_room = if glass_at_end { 30.0 } else { 10.0 + timing_w };
        // Exactly one line tall with no padding, so the letters land in the middle of the bar.
        let font = FontId::proportional(font_size);
        let line_height = line_galley(ui, "Ag", font.clone(), pal.text, 100.0).size().y;
        let top = rect.center().y - line_height / 2.0 - ink_nudge(font_size);
        let edit_rect = Rect::from_min_max(pos2(text_left, top), pos2(rect.right() - end_room, top + line_height));
        let before = self.query.clone();
        let hint_text = match style {
            Experience::Windows11 => "Search for apps, settings, and documents",
            Experience::Windows10 => "Type here to search",
            Experience::Windows7 => "Search programs and files",
            Experience::WindowsXp => "Search",
            Experience::Startline => "Search",
        };
        let hint = RichText::new(hint_text).color(pal.sub).size(font_size);
        let edit = TextEdit::singleline(&mut self.query)
            .id(Id::unique("search"))
            .frame(Frame::NONE)
            .margin(Margin::ZERO)
            .hint_text(hint)
            .font(font)
            .text_color(pal.text)
            .desired_width(edit_rect.width());
        let response = ui.put(edit_rect, edit);
        if self.popup.is_none() && (self.focus_search || !response.has_focus()) {
            response.request_focus();
            self.focus_search = false;
        }
        if self.query != before {
            self.query_changed();
        }
    }

    fn timing_label(&self) -> String {
        if !self.cfg.show_timing || self.query.trim().is_empty() {
            return String::new();
        }
        let micros = self.file_micros.unwrap_or(0) + self.fast_micros;
        if micros < 1000 {
            format!("{micros} µs")
        } else {
            format!("{:.1} ms", micros as f32 / 1000.0)
        }
    }

    fn draw_home(&mut self, ui: &mut Ui, pal: Pal) {
        if !self.cfg.seen_welcome {
            self.draw_welcome(ui, pal);
        }
        for section in self.cfg.sections.clone() {
            if !section.visible {
                continue;
            }
            match section.kind {
                SectionKind::Pinned => self.draw_pinned(ui, pal),
                SectionKind::Recent => self.draw_recent(ui, pal),
                SectionKind::Frequent => self.draw_frequent(ui, pal),
                SectionKind::AllApps => {
                    section_title(ui, "All apps", pal);
                    self.draw_app_list(ui, pal);
                }
            }
            ui.add_space(8.0);
        }
    }

    fn draw_welcome(&mut self, ui: &mut Ui, pal: Pal) {
        Frame::new().fill(pal.surface).corner_radius(pal.radius + 2).inner_margin(12).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("This is your new Start menu").font(semibold(14.0)).color(pal.text));
            ui.add_space(2.0);
            let text = if self.cfg.take_over_start {
                "The Start key and Start button open it now. Shift + Start still opens the Windows one. Pick another look any time in Settings."
            } else {
                "Pick another look any time in Settings."
            };
            ui.label(RichText::new(text).size(13.0).color(pal.sub));
            ui.add_space(6.0);
            if pill_button(ui, "Got it", pal, true).clicked() {
                self.cfg.seen_welcome = true;
            }
        });
        ui.add_space(10.0);
    }

    /// Columns and tile height. Only Startline's are free to change.
    fn tile_grid(&self) -> (usize, f32) {
        match self.look.experience {
            Experience::Startline => {
                let height = if self.cfg.show_labels { self.cfg.tile_size } else { self.cfg.tile_size * 0.72 };
                (self.cfg.columns.max(2), height)
            }
            Experience::Windows10 => (3, 100.0),
            _ => (6, 84.0),
        }
    }

    pub(crate) fn draw_pinned(&mut self, ui: &mut Ui, pal: Pal) {
        let pointer_up = ui.input(|i| i.pointer.any_released());
        let groups = self.cfg.groups.clone();
        let mut any = false;
        for (group_index, group) in groups.iter().enumerate() {
            if group.pins.is_empty() && groups.len() > 1 {
                continue;
            }
            any |= !group.pins.is_empty();
            let title_top = ui.cursor().min.y;
            section_title(ui, &group.name, pal);
            if pal.style == Experience::Windows11 && group_index == 0 {
                let spot = Rect::from_min_size(pos2(ui.max_rect().right() - 70.0, title_top + 1.0), vec2(70.0, 24.0));
                let all = ui.interact(spot, Id::unique("win11-all"), Sense::click());
                ui.painter().rect_filled(spot, pal.radius, if all.hovered() { pal.hover } else { pal.surface });
                ui.painter().text(spot.center() - vec2(6.0, ink_nudge(12.5)), Align2::CENTER_CENTER, "All", FontId::proportional(12.5), pal.text);
                ui.painter().text(pos2(spot.right() - 14.0, spot.center().y), Align2::CENTER_CENTER, '\u{E76C}', icon_font(9.0), pal.text);
                if all.clicked() {
                    self.intents.push(Intent::Show(Page::AllApps));
                }
            }
            let (columns, tile_h) = self.tile_grid();
            let tile_w = ui.available_width() / columns as f32;
            for (row_index, row) in group.pins.chunks(columns).enumerate() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    for (col, pin) in row.iter().enumerate() {
                        let pin_index = row_index * columns + col;
                        self.draw_tile(ui, vec2(tile_w, tile_h), pin, (group_index, pin_index), pal);
                    }
                });
            }
        }
        if !any && groups.len() <= 1 {
            ui.label(RichText::new("Right-click any app or file to pin it here.").size(13.0).color(pal.sub));
        }
        if pointer_up {
            self.drag_pin = None;
        }
    }

    fn draw_tile(&mut self, ui: &mut Ui, size: Vec2, pin: &Pin, slot: (usize, usize), pal: Pal) {
        let id = Id::unique(("pin", slot.0, &pin.target));
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let response = ui.interact(rect.shrink(2.0), id, Sense::click_and_drag());
        let intent = Intent::Open { name: pin.name.clone(), target: pin.target.clone(), admin: false };
        self.nav_next.push(NavEntry { id, rect, kind: NavKind::Intent(intent.clone()) });

        let is_selected = self.selected == Some(id);
        if is_selected && self.scroll_to_selected {
            ui.scroll_to_rect(rect, None);
            self.scroll_to_selected = false;
        }
        if response.dragged() || response.drag_started() {
            self.drag_pin.get_or_insert(slot);
        }
        // Dragging over another tile moves the pin there.
        if let Some((from_group, from)) = self.drag_pin {
            let over = ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| rect.contains(p));
            if over && (from_group, from) != slot && from < self.cfg.groups[from_group].pins.len() {
                let moved = self.cfg.groups[from_group].pins.remove(from);
                let at = slot.1.min(self.cfg.groups[slot.0].pins.len());
                self.cfg.groups[slot.0].pins.insert(at, moved);
                self.drag_pin = Some((slot.0, at));
            }
        }
        let dragging_this = self.drag_pin == Some(slot);
        // Windows 10's pins are solid colored tiles with the name in the corner.
        let colored_tile = pal.style == Experience::Windows10;
        if colored_tile {
            let lit = response.hovered() || is_selected || dragging_this;
            ui.painter().rect_filled(rect.shrink(2.0), 0, if lit { mix(pal.accent, Color32::WHITE, 0.16) } else { pal.accent });
        } else if response.hovered() || is_selected || dragging_this {
            let fill = if is_selected || dragging_this { pal.selected } else { pal.hover };
            ui.painter().rect_filled(rect.shrink(2.0), pal.radius + 2, fill);
        }
        let show_labels = self.cfg.show_labels || pal.style != Experience::Startline;
        let icon_size = (size.y * 0.46).clamp(20.0, 72.0);
        let label_room = if show_labels { 22.0 } else { 0.0 };
        let top = rect.center().y - (icon_size + label_room) / 2.0;
        let icon_rect = Rect::from_center_size(pos2(rect.center().x, top + icon_size / 2.0), Vec2::splat(icon_size));
        self.draw_icon(ui, icon_rect, &Icon::for_target_cached(&pin.target), &pin.name, pal);
        if colored_tile {
            let galley = line_galley(ui, &pin.name, FontId::proportional(11.5), pal.on_accent, rect.width() - 12.0);
            ui.painter().galley(pos2(rect.left() + 6.0, rect.bottom() - galley.size().y - 5.0), galley, pal.on_accent);
        } else if show_labels {
            let galley = line_galley(ui, &pin.name, FontId::proportional(12.5), pal.text, rect.width() - 8.0);
            let pos = pos2(rect.center().x - galley.size().x / 2.0, top + icon_size + 6.0);
            ui.painter().galley(pos, galley, pal.text);
        }
        if response.clicked() {
            self.intents.push(intent.clone());
        }
        if response.secondary_clicked() {
            let path = self.path_of_target(&pin.target);
            let entries = popup::target_entries(&self.cfg, &pin.name, &pin.target, path.as_deref(), true);
            let at = response.interact_pointer_pos().unwrap_or(rect.center());
            self.open_popup(at, entries);
        }
    }

    pub(crate) fn draw_icon(&mut self, ui: &Ui, rect: Rect, icon: &Icon, name: &str, pal: Pal) {
        if !ui.is_rect_visible(rect) {
            return;
        }
        match icon {
            Icon::Image { key, source } => match self.icons.get(key, source) {
                Some(texture) => {
                    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                    ui.painter().image(texture, rect, uv, Color32::WHITE);
                }
                None => {
                    let letter = name.chars().next().unwrap_or('?').to_uppercase().to_string();
                    ui.painter().rect_filled(rect, rect.width() * 0.22, mix(pal.surface, pal.text, 0.10));
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, letter, semibold(rect.height() * 0.5), pal.sub);
                }
            },
            Icon::Glyph(glyph) => {
                ui.painter().rect_filled(rect, rect.width() * 0.22, mix(pal.surface, pal.accent, 0.20));
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, *glyph, icon_font(rect.height() * 0.56), pal.text);
            }
        }
    }


    /// One setting in the results: its icon and name, how it stands, and a live control.
    fn draw_setting_row(&mut self, ui: &mut Ui, row: &Row, id: QuickId, want: Want, width: f32, pal: Pal) {
        let info = id.info();
        let value = self.settings.get(id);
        let compact = self.cfg.compact_rows;
        let (rect, _) = ui.allocate_exact_size(vec2(width, if compact { 38.0 } else { 50.0 }), Sense::hover());
        let nav_id = Id::unique(("quick", id));
        let response = ui.interact(rect, nav_id, Sense::click());
        self.nav_next.push(NavEntry { id: nav_id, rect, kind: NavKind::Quick(id, want) });
        let selected = self.selected == Some(nav_id);
        if selected && self.scroll_to_selected {
            ui.scroll_to_rect(rect, None);
            self.scroll_to_selected = false;
        }
        if !ui.is_rect_visible(rect) {
            return;
        }
        // The older menus light a row up the moment the pointer is on it.
        let lit = selected || (response.hovered() && pal.style.is_classic());
        if lit {
            paint_selection(ui, rect, pal);
        } else if response.hovered() {
            ui.painter().rect_filled(rect, pal.radius, pal.hover);
        }
        let (name_ink, sub_ink) = if lit && pal.style == Experience::WindowsXp { (pal.on_accent, pal.on_accent) } else { (pal.text, pal.sub) };
        let center = rect.center().y;
        let icon_size = if compact { 20.0 } else { 26.0 };
        let icon_rect = Rect::from_center_size(pos2(rect.left() + ICON_COLUMN, center), Vec2::splat(icon_size));
        self.draw_icon(ui, icon_rect, &row.icon, &row.name, pal);

        let right = rect.right() - 12.0;
        let control_left = match (info.control, value) {
            (Control::Toggle, Value::On(_)) => right - 40.0,
            (Control::Slider { .. }, Value::Number(_)) => right - (rect.width() * 0.38).min(240.0),
            (Control::Choice(options), Value::Pick(_)) => right - options.len() as f32 * 72.0,
            _ => right,
        };
        let text_left = rect.left() + TEXT_COLUMN;
        let text_width = (control_left - text_left - 12.0).max(40.0);
        let mark = if lit && pal.style == Experience::WindowsXp { pal.on_accent } else { pal.accent };
        let name = highlighted_galley(ui, &row.name, &self.subject, FontId::proportional(14.0), name_ink, mark, text_width);
        let mut line = intent::hint(info.control, value, want);
        if info.late && !matches!(value, Value::Unavailable | Value::Loading) {
            line.push_str("  ·  May need a sign-out to show");
        }
        if compact {
            ui.painter().galley(pos2(text_left, center - name.size().y / 2.0 - ink_nudge(14.0)), name, name_ink);
        } else {
            let detail = line_galley(ui, &line, FontId::proportional(12.0), sub_ink, text_width);
            let top = center - (name.size().y + detail.size().y + 1.0) / 2.0;
            ui.painter().galley(pos2(text_left, top + name.size().y + 1.0), detail, sub_ink);
            ui.painter().galley(pos2(text_left, top), name, name_ink);
        }

        // Drawn after the row, so a press on the control belongs to the control.
        match (info.control, value) {
            (Control::Toggle, Value::On(on)) => {
                let switch_rect = Rect::from_center_size(pos2(right - 20.0, center), vec2(40.0, 22.0));
                let clicked = ui.interact(switch_rect.expand(4.0), nav_id.with("switch"), Sense::click()).clicked();
                draw_switch(ui, switch_rect, on, pal);
                if clicked {
                    self.settings.set(id, Value::On(!on));
                }
            }
            (Control::Slider { min, max, step, unit }, Value::Number(current)) => {
                let text = intent::format_number(current, unit);
                let galley = line_galley(ui, &text, FontId::proportional(12.5), pal.sub, 56.0);
                ui.painter().galley(pos2(right - galley.size().x, center - galley.size().y / 2.0 - ink_nudge(12.5)), galley, pal.sub);
                let track = Rect::from_min_max(pos2(control_left, rect.top() + 6.0), pos2(right - 60.0, rect.bottom() - 6.0));
                if let Some(new_value) = draw_slider(ui, track, nav_id.with("slider"), current, min, max, step, pal) {
                    self.settings.set(id, Value::Number(new_value));
                }
            }
            (Control::Choice(options), Value::Pick(current)) => {
                let area = Rect::from_min_max(pos2(control_left, center - 14.0), pos2(right, center + 14.0));
                if let Some(pick) = segmented_at(ui, area, nav_id.with("choice"), options, current, pal) {
                    self.settings.set(id, Value::Pick(pick));
                }
            }
            _ => {}
        }
        if response.clicked() {
            self.activate_quick(id, want);
        }
        if response.secondary_clicked() {
            let at = response.interact_pointer_pos().unwrap_or(rect.center());
            self.open_popup(at, popup::setting_entries(info.page));
        }
    }

    fn draw_recent(&mut self, ui: &mut Ui, pal: Pal) {
        let list = self.recent.lock().unwrap().clone();
        section_title(ui, if pal.style == Experience::Windows11 { "Recommended" } else { "Recent files" }, pal);
        if list.is_empty() {
            ui.label(RichText::new("Files you open will show up here.").size(13.0).color(pal.sub));
            return;
        }
        let now = platform::unix_now();
        let gap = 6.0;
        let width = (ui.available_width() - gap) / 2.0;
        for pair in list.chunks(2) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(gap, 0.0);
                for (name, path, mtime) in pair {
                    let row = Row {
                        category: Category::Files,
                        name: name.clone(),
                        detail: relative_time(now.saturating_sub(*mtime)),
                        icon: Icon::Image { key: icons::file_icon_key(path, false), source: path.clone() },
                        kind: RowKind::Open(Target::path(path.clone())),
                        path: Some(path.clone()),
                    };
                    self.draw_row(ui, &row, width, pal, true);
                }
            });
        }
    }

    fn draw_frequent(&mut self, ui: &mut Ui, pal: Pal) {
        let mut items: Vec<_> = self.cfg.usage.values().filter(|u| u.target.is_some() && u.count > 0).cloned().collect();
        items.sort_by_key(|u| std::cmp::Reverse((u.count, u.last)));
        items.truncate(self.cfg.frequent_count.max(1));
        section_title(ui, "Most used", pal);
        if items.is_empty() {
            ui.label(RichText::new("What you open most will show up here.").size(13.0).color(pal.sub));
            return;
        }
        let gap = 6.0;
        let width = (ui.available_width() - gap) / 2.0;
        for pair in items.chunks(2) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(gap, 0.0);
                for usage in pair {
                    let target = usage.target.clone().unwrap_or_else(|| Target::path(""));
                    let row = Row {
                        category: Category::Apps,
                        name: usage.name.clone(),
                        detail: format!("Opened {} times", usage.count),
                        icon: Icon::for_target_cached(&target),
                        kind: RowKind::Open(target),
                        path: None,
                    };
                    self.draw_row(ui, &row, width, pal, true);
                }
            });
        }
    }

    fn draw_all_apps_page(&mut self, ui: &mut Ui, pal: Pal) {
        // The title belongs on the left and the way out on the right, as in the Windows menu.
        ui.horizontal(|ui| {
            ui.label(RichText::new("All apps").font(semibold(14.0)).color(pal.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if pill_button(ui, "Back", pal, false).clicked() {
                    self.intents.push(Intent::Show(Page::Home));
                }
            });
        });
        ui.add_space(4.0);
        self.draw_app_list(ui, pal);
    }

    pub(crate) fn draw_app_list(&mut self, ui: &mut Ui, pal: Pal) {
        let apps = self.apps.read().unwrap().clone();
        let mut letter = None;
        let width = ui.available_width();
        for app in apps.iter() {
            let first = app.name.chars().next().map(|c| c.to_uppercase().next().unwrap_or(c));
            let first = first.filter(|c| c.is_alphabetic()).unwrap_or('#');
            if letter != Some(first) {
                letter = Some(first);
                ui.add_space(4.0);
                ui.label(RichText::new(first.to_string()).font(semibold(12.5)).color(pal.accent));
            }
            let target = Target::App(app.id.clone());
            let row = Row {
                category: Category::Apps,
                name: app.name.clone(),
                detail: String::new(),
                icon: Icon::for_target_cached(&target),
                kind: RowKind::Open(target),
                path: (!app.path.is_empty()).then(|| app.path.clone()),
            };
            self.draw_row(ui, &row, width, pal, false);
        }
        if apps.is_empty() {
            ui.label(RichText::new("Reading your apps...").size(13.0).color(pal.sub));
        }
    }

    /// How much room the file type buttons take, right beside the search bar.
    pub(crate) fn chip_row_height(&self) -> f32 {
        if self.policy.file_buttons() && !self.query.trim().is_empty() && (self.facets.total > 0 || self.filtering()) { 32.0 } else { 0.0 }
    }

    /// One row of buttons for the kinds of file the words matched. It stays in one place right
    /// next to the search bar, and whatever is switched on comes first.
    pub(crate) fn draw_filter_chips(&mut self, ui: &mut Ui, rect: Rect, pal: Pal) {
        #[derive(Clone)]
        enum Press {
            Kind(&'static [&'static str]),
            Ext(String),
            Folders,
            Clear,
        }
        if self.chip_row_height() == 0.0 {
            return;
        }
        let facets = self.facets.clone();
        let typed = self.typed_filters();
        let filter = self.file_filter.clone();
        let count_of = |ext: &str| facets.exts.iter().find(|(e, _)| e == ext).map_or(0, |(_, n)| *n);
        let is_on = |ext: &str| typed.iter().any(|t| t == ext) || filter.exts.iter().any(|e| e == ext);

        let mut chips: Vec<(String, Option<u32>, bool, Press)> = Vec::new();
        if facets.folders > 0 || filter.folders {
            chips.push(("Folders".to_string(), Some(facets.folders), filter.folders, Press::Folders));
        }
        let mut in_kinds: Vec<&str> = Vec::new();
        for (label, exts) in FILE_KINDS {
            let all_on = exts.iter().all(|e| filter.exts.iter().any(|on| on == e));
            let count: u32 = exts.iter().map(|e| count_of(e)).sum();
            if all_on {
                in_kinds.extend(exts.iter().copied());
            }
            if count > 0 || all_on {
                chips.push((label.to_string(), Some(count), all_on, Press::Kind(exts)));
            }
        }
        for (ext, count) in &facets.exts {
            if !in_kinds.contains(&ext.as_str()) {
                chips.push((ext.clone(), Some(*count), is_on(ext), Press::Ext(ext.clone())));
            }
        }
        for ext in typed.iter().chain(filter.exts.iter()) {
            if !in_kinds.contains(&ext.as_str()) && !chips.iter().any(|c| c.0 == *ext) {
                chips.push((ext.clone(), Some(0), true, Press::Ext(ext.clone())));
            }
        }
        chips.sort_by_key(|c| !c.2);

        let filtering = self.filtering();
        let clear_room = if filtering { chip_width(ui, "Clear", None) + 6.0 } else { 0.0 };
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
        child.set_clip_rect(rect);
        child.spacing_mut().item_spacing = vec2(5.0, 0.0);
        let mut used = 0.0;
        let mut press = None;
        for (label, count, on, action) in &chips {
            let width = chip_width(&child, label, *count);
            if used + width + clear_room > rect.width() {
                break;
            }
            used += width + 5.0;
            if chip(&mut child, label, *count, *on, pal).clicked() {
                press = Some(action.clone());
            }
        }
        if filtering && chip(&mut child, "Clear", None, false, pal).clicked() {
            press = Some(Press::Clear);
        }
        match press {
            Some(Press::Kind(exts)) => self.toggle_kind_filter(exts),
            Some(Press::Ext(ext)) => self.toggle_ext_filter(&ext),
            Some(Press::Folders) => self.toggle_folder_filter(),
            Some(Press::Clear) => self.clear_filters(),
            None => {}
        }
    }

    pub(crate) fn draw_results(&mut self, ui: &mut Ui, pal: Pal) {
        let rows = std::mem::take(&mut self.rows);
        let width = ui.available_width();
        if rows.is_empty() {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(format!("Nothing found for \"{}\"", self.query.trim())).size(14.0).color(pal.sub));
            });
        }
        let mut last = None;
        for row in &rows {
            if last != Some(row.category) {
                last = Some(row.category);
                section_title(ui, row.category.label(), pal);
            }
            if let RowKind::Quick(id, want) = row.kind {
                self.draw_setting_row(ui, row, id, want, width, pal);
            } else {
                self.draw_row(ui, row, width, pal, self.cfg.show_paths || row.category != Category::Files);
            }
        }
        self.rows = rows;
    }

    /// One line in a list: icon, name, detail.
    pub(crate) fn draw_row(&mut self, ui: &mut Ui, row: &Row, width: f32, pal: Pal, show_detail: bool) {
        let compact = self.cfg.compact_rows;
        let two_lines = !row.detail.is_empty() && !compact && show_detail;
        let height = if compact {
            32.0
        } else if two_lines {
            46.0
        } else {
            38.0
        };
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        let id = Id::unique(("row", &row.name, &row.kind_key()));
        let response = ui.interact(rect, id, Sense::click());
        let intent = match &row.kind {
            RowKind::Open(target) => Some(Intent::Open { name: row.name.clone(), target: target.clone(), admin: false }),
            RowKind::Run(what) => Some(Intent::Run(*what)),
            RowKind::Web(url) => Some(Intent::Web(url.clone())),
            RowKind::Quick(..) => None,
        };
        if let Some(intent) = &intent {
            self.nav_next.push(NavEntry { id, rect, kind: NavKind::Intent(intent.clone()) });
        }
        let selected = self.selected == Some(id);
        if selected && self.scroll_to_selected {
            ui.scroll_to_rect(rect, None);
            self.scroll_to_selected = false;
        }
        if !ui.is_rect_visible(rect) {
            return;
        }
        // The older menus light a row up the moment the pointer is on it.
        let lit = selected || (response.hovered() && pal.style.is_classic());
        if lit {
            paint_selection(ui, rect, pal);
        } else if response.hovered() {
            ui.painter().rect_filled(rect, pal.radius, pal.hover);
        }
        let (name_ink, sub_ink) = if lit && pal.style == Experience::WindowsXp { (pal.on_accent, pal.on_accent) } else { (pal.text, pal.sub) };
        let icon_size = if compact { 20.0 } else { 26.0 };
        let icon_rect = Rect::from_center_size(pos2(rect.left() + ICON_COLUMN, rect.center().y), Vec2::splat(icon_size));
        self.draw_icon(ui, icon_rect, &row.icon, &row.name, pal);

        let text_left = rect.left() + TEXT_COLUMN;
        let text_width = rect.right() - text_left - 8.0;
        let confirming = matches!(row.kind, RowKind::Run(what) if self.confirm.is_some_and(|(d, at)| d == what && at.elapsed() < Duration::from_secs(5)));
        let shown_name = if confirming { format!("Press again to confirm: {}", row.name) } else { row.name.clone() };
        let name_color = if confirming && !lit { pal.accent } else { name_ink };
        let mark = if lit && pal.style == Experience::WindowsXp { pal.on_accent } else { pal.accent };
        let name = highlighted_galley(ui, &shown_name, &self.tokens, FontId::proportional(14.0), name_color, mark, text_width);
        if two_lines {
            let detail = path_galley(ui, &row.detail, FontId::proportional(12.0), sub_ink, text_width);
            let total = name.size().y + detail.size().y + 1.0;
            let top = rect.center().y - total / 2.0;
            ui.painter().galley(pos2(text_left, top + name.size().y + 1.0), detail, sub_ink);
            ui.painter().galley(pos2(text_left, top), name, name_ink);
        } else {
            ui.painter().galley(pos2(text_left, rect.center().y - name.size().y / 2.0 - ink_nudge(14.0)), name, name_ink);
        }
        if response.clicked() {
            if let Some(intent) = intent {
                self.intents.push(intent);
            }
        }
        if let (RowKind::Open(target), true) = (&row.kind, response.secondary_clicked()) {
            let path = row.path.clone().or_else(|| self.path_of_target(target));
            let entries = popup::target_entries(&self.cfg, &row.name, target, path.as_deref(), false);
            let at = response.interact_pointer_pos().unwrap_or(rect.center());
            self.open_popup(at, entries);
        }
    }

    /// The account picture in a circle, or your initial until it has loaded.
    pub(crate) fn draw_avatar(&mut self, ui: &Ui, rect: Rect, pal: Pal) {
        let path = self.user_picture.lock().unwrap().clone();
        let texture = path.and_then(|path| self.icons.get("picture:account", &path));
        match texture {
            Some(id) => {
                let sized = load::SizedTexture::new(id, rect.size());
                Image::from_texture(sized).corner_radius(rect.width() / 2.0).paint_at(ui, rect);
            }
            None => {
                let letter = platform::user_name().chars().next().unwrap_or('?').to_uppercase().to_string();
                ui.painter().circle_filled(rect.center(), rect.width() / 2.0, mix(pal.surface, pal.text, 0.18));
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, letter, semibold(rect.height() * 0.46), pal.text);
            }
        }
    }

    /// Opens the account menu above or beside what was clicked.
    pub(crate) fn open_user_menu(&mut self, anchor: Rect) {
        let entries = popup::user_entries();
        let height = popup::height_of(&entries);
        self.open_popup(pos2(anchor.left(), anchor.top() - height - 4.0), entries);
    }

    /// Opens the power menu above the button that was clicked.
    pub(crate) fn open_power_menu(&mut self, anchor: Rect) {
        let item = |glyph, label: &str, what| Entry::Item { glyph, label: label.to_string(), intent: Intent::Run(what) };
        let entries = vec![item('\u{E708}', "Sleep", Do::Sleep), item('\u{E7E8}', "Shut down", Do::ShutDown), item('\u{E777}', "Restart", Do::Restart)];
        let height = popup::height_of(&entries);
        self.open_popup(pos2(anchor.right() - 248.0, anchor.top() - height - 4.0), entries);
    }

    fn draw_footer(&mut self, ui: &mut Ui, rect: Rect, pal: Pal) {
        // In the middle of the darker band in Windows 11, which reaches down to the window edge.
        let center = rect.center().y + if pal.style == Experience::Windows11 { 5.0 } else { 0.0 };
        if !self.query.trim().is_empty() {
            let on_setting = self.selected_entry().is_some_and(|e| matches!(e.kind, NavKind::Quick(..)));
            let hint = if on_setting {
                "Enter applies   ·   Down, then left and right adjusts   ·   Right-click for more   ·   Esc clears".to_string()
            } else {
                let admin = if self.policy.allows("Menu_RunAsAdmin") { "Ctrl+Enter as admin   ·   " } else { "" };
                let folder = if self.policy.allows("Menu_OpenLocation") { "Shift+Enter shows in folder   ·   " } else { "" };
                format!("Enter opens   ·   {admin}{folder}Esc clears")
            };
            let galley = line_galley(ui, &hint, FontId::proportional(12.0), pal.sub, rect.width());
            ui.painter().galley(pos2(rect.left() + 4.0, center - galley.size().y / 2.0 - ink_nudge(12.0)), galley, pal.sub);
            return;
        }
        // Your picture and name are one button, opening what can be done with your account.
        let status = self.status_text();
        let label = if status.is_empty() { platform::user_name() } else { status };
        let galley = line_galley(ui, &label, FontId::proportional(13.0), pal.text, rect.width() - 180.0);
        let button = Rect::from_min_size(pos2(rect.left(), center - 18.0), vec2(galley.size().x + TEXT_COLUMN + 12.0, 36.0));
        let user = ui.interact(button, Id::unique("user-button"), Sense::click()).on_hover_text("Account");
        if user.hovered() {
            ui.painter().rect_filled(button, pal.radius, pal.hover);
        }
        let avatar = Rect::from_center_size(pos2(button.left() + ICON_COLUMN, center), Vec2::splat(28.0));
        self.draw_avatar(ui, avatar, pal);
        ui.painter().galley(pos2(button.left() + TEXT_COLUMN, center - galley.size().y / 2.0 - ink_nudge(13.0)), galley, pal.text);
        if user.clicked() {
            self.open_user_menu(button);
        }

        let button_size = 34.0;
        let mut x = rect.right() - button_size;
        let mut place = |ui: &mut Ui, id: &str, glyph: char, tip: &str, active: bool| {
            let area = Rect::from_center_size(pos2(x + button_size / 2.0, center), Vec2::splat(button_size));
            x -= button_size + 4.0;
            icon_button(ui, area, Id::unique(id), glyph, tip, active, pal)
        };
        let can_power = [Do::Sleep, Do::ShutDown, Do::Restart].into_iter().any(|what| self.policy.allows_do(what));
        let power = if can_power { Some(place(ui, "power", '\u{E7E8}', "Power", false)) } else { None };
        let gear = if self.policy.allows_do(Do::Customize) { Some(place(ui, "customize", '\u{E713}', "Settings", false)) } else { None };
        // Windows 11 keeps this as the "All" button above the pinned apps.
        if pal.style == Experience::Startline {
            let all = place(ui, "all", '\u{E71D}', "All apps", self.page == Page::AllApps);
            if all.clicked() {
                let next = if self.page == Page::AllApps { Page::Home } else { Page::AllApps };
                self.intents.push(Intent::Show(next));
            }
        }
        if let Some(power) = power.filter(|p| p.clicked()) {
            self.open_power_menu(power.rect);
        }
        if gear.is_some_and(|g| g.clicked()) {
            self.intents.push(Intent::Show(Page::Customize(Tab::Appearance)));
        }
    }

    fn status_text(&self) -> String {
        let status = &self.index.status;
        if status.walking.load(Ordering::Relaxed) && self.file_count() < 1000 {
            let walked = status.walked.load(Ordering::Relaxed);
            return format!("Reading your folders... {} so far", thousands(walked));
        }
        String::new()
    }
}

