//! Drawing the menu's screens: search bar, home sections, results and footer.

use crate::actions::Do;
use crate::config::{Category, Pin, SectionKind, Target};
use crate::customize::Tab;
use crate::i18n::{tr, tx};
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
        // Over Mica the window is clear: Windows paints the material, already tinted for light or dark.
        let alpha = if look.mica && (!in_settings || style == Experience::Startline) { 0 } else { (opacity.clamp(0.25, 1.0) * 255.0) as u8 };
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
        // The calendar panel takes the right of the window; the menu keeps the width it always has.
        let full = if crate::m365::panel_shown(&self.cfg) && full.width() > crate::m365::PANEL_WIDTH + 320.0 {
            let split = full.right() - crate::m365::PANEL_WIDTH;
            self.draw_calendar(ui, Rect::from_min_max(pos2(split, full.top()), full.max), pal);
            Rect::from_min_max(full.min, pos2(split, full.bottom()))
        } else {
            full
        };
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
        // What's playing sits just above the footer.
        let playing = self.now_playing();
        let media_room = if playing.is_empty() { 0.0 } else { crate::now_playing::CARD_HEIGHT + gap };
        let above = footer.top() - media_room;
        let (search_rect, chip_rect, body) = if search_low {
            let search = Rect::from_min_max(pos2(content.left(), above - gap - search_h), pos2(content.right(), above - gap));
            let chips = Rect::from_min_max(pos2(content.left(), search.top() - chip_room), pos2(content.right(), search.top() - 6.0));
            (search, chips, Rect::from_min_max(content.left_top(), pos2(content.right(), search.top() - chip_room - gap)))
        } else {
            let search = Rect::from_min_size(content.left_top(), vec2(content.width(), search_h));
            let chips = Rect::from_min_max(pos2(content.left(), search.bottom() + 6.0), pos2(content.right(), search.bottom() + chip_room));
            (search, chips, Rect::from_min_max(pos2(content.left(), search.bottom() + chip_room + gap), pos2(content.right(), above - gap)))
        };
        if !playing.is_empty() {
            let card = Rect::from_min_max(pos2(content.left(), above), pos2(content.right(), footer.top() - gap));
            self.draw_now_playing(ui, card, &playing, pal);
        }

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
            Experience::Startline => (CornerRadius::same(pal.radius), pal.field, pal.field),
            Experience::Windows11 => (CornerRadius::same((rect.height() / 2.0) as u8), pal.field, pal.line),
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
        // From the right: the help button, the regular expression switch, then how long the search took.
        let mut x = rect.right() - if glass_at_end { 30.0 } else { 8.0 };
        let size = (rect.height() - 10.0).clamp(18.0, 28.0);
        let slot = |x: &mut f32| {
            *x -= size;
            let area = Rect::from_center_size(pos2(*x + size / 2.0, rect.center().y), Vec2::splat(size));
            *x -= 2.0;
            area
        };
        if self.cfg.show_search_help {
            let area = slot(&mut x);
            let response = ui.interact(area, Id::unique("search-help"), Sense::click());
            self.paint_box_button(ui, area, response.hovered(), self.help_open, pal);
            ui.painter().text(area.center(), Align2::CENTER_CENTER, '\u{E897}', icon_font(size * 0.5), pal.sub);
            if response.on_hover_text(tr("What you can type")).clicked() {
                self.help_open = !self.help_open;
            }
        }
        if self.cfg.show_regex_switch {
            let area = slot(&mut x);
            let on = self.cfg.regex_search;
            let response = ui.interact(area, Id::unique("search-regex"), Sense::click());
            self.paint_box_button(ui, area, response.hovered(), on, pal);
            let ink = if on { pal.accent } else { pal.sub };
            ui.painter().text(area.center() - vec2(0.0, ink_nudge(12.0)), Align2::CENTER_CENTER, ".*", FontId::monospace(12.0), ink);
            let tip = if on { tr("Regular expression: on. The whole search is one pattern.") } else { tr("Regular expression: off") };
            if response.on_hover_text(tip).clicked() {
                self.cfg.regex_search = !on;
                self.query_changed();
            }
        }
        let timing = if glass_at_end || style != Experience::Startline { String::new() } else { self.timing_label() };
        if !timing.is_empty() {
            let galley = line_galley(ui, &timing, FontId::proportional(12.0), pal.sub, 200.0);
            let width = galley.size().x;
            ui.painter().galley(pos2(x - width - 6.0, rect.center().y - galley.size().y / 2.0 - ink_nudge(12.0)), galley, pal.sub);
            x -= width + 12.0;
        }
        let end_room = rect.right() - x + 4.0;
        // Exactly one line tall with no padding, so the letters land in the middle of the bar.
        let font = FontId::proportional(font_size);
        let line_height = line_galley(ui, "Ag", font.clone(), pal.text, 100.0).size().y; // i18n-ignore: sample letters to measure a line
        let top = rect.center().y - line_height / 2.0 - ink_nudge(font_size);
        let edit_rect = Rect::from_min_max(pos2(text_left, top), pos2(rect.right() - end_room, top + line_height));
        let before = self.query.clone();
        let hint_text = match style {
            Experience::Windows11 => tx("Search for apps, settings, and documents"),
            Experience::Windows10 => tx("Type here to search"),
            Experience::Windows7 => tx("Search programs and files"),
            Experience::WindowsXp => tx("Search"),
            Experience::Startline => tx("Search"),
        };
        let hint_text = if self.regex_on() { tr("Regular expression") } else { tr(hint_text) };
        // A hint longer in another language than the box is wide gets a little smaller rather than cut off.
        let needed = ui.painter().layout_no_wrap(hint_text.to_string(), FontId::proportional(font_size), pal.sub).size().x;
        let hint_size = if needed > edit_rect.width() { (font_size * edit_rect.width() / needed).max(font_size * 0.8) } else { font_size };
        let hint = RichText::new(hint_text).color(pal.sub).size(hint_size);
        let edit = TextEdit::singleline(&mut self.query)
            .id(Id::unique("search"))
            .frame(Frame::NONE)
            .margin(Margin::ZERO)
            .hint_text(hint)
            .font(font)
            .text_color(pal.text)
            .desired_width(edit_rect.width());
        let response = text_box(ui, Some(edit_rect), edit);
        if self.options.demo_text_menu && self.is_shot() {
            let menu_id = response.id.with("popup");
            if !Popup::is_id_open(ui.ctx(), menu_id) {
                let mut state = TextEdit::load_state(ui.ctx(), response.id).unwrap_or_default();
                state.cursor.set_char_range(Some(text::CCursorRange::select_all_str(&self.query)));
                TextEdit::store_state(ui.ctx(), response.id, state);
                Popup::open_id(ui.ctx(), menu_id);
            }
        }
        if self.popup.is_none() && (self.focus_search || !response.has_focus()) {
            response.request_focus();
            self.focus_search = false;
        }
        if self.query != before {
            self.query_changed();
        }
        if self.help_open {
            self.draw_search_help(ui, rect, pal);
        }
    }

    /// A small square button in the search box, lit while its thing is on.
    fn paint_box_button(&self, ui: &Ui, area: Rect, hovered: bool, on: bool, pal: Pal) {
        if on {
            ui.painter().rect_filled(area, pal.radius.min(4), mix(pal.field, pal.accent, 0.18));
        } else if hovered {
            ui.painter().rect_filled(area, pal.radius.min(4), pal.hover);
        }
    }

    /// What can be typed in the search box, in a panel beside it. Closes with a click elsewhere or Esc.
    fn draw_search_help(&mut self, ui: &Ui, anchor: Rect, pal: Pal) {
        const LINES: [(&str, &str); 9] = [
            ("report final", tx("Names holding every word, or the other words naming a folder they're in")), // i18n-ignore: search syntax
            ("\"exact text\"", tx("These words together, in this order")), // i18n-ignore: search syntax
            ("*.pdf   img_??.png", tx("Wildcards: * any letters, ? one letter")),
            ("-draft", tx("Leave out names holding this")),
            ("ext:pdf,docx", tx("Only these file types")),
            ("kind:picture", tx("folder, document, picture, video, music, archive, code or app")),
            ("size:>10mb   size:1mb..1gb", tx("Bigger, smaller or in between")),
            ("modified:today   date:2025-03", tx("today, yesterday, week, month, year, a year, a month, a day, >2026-01-01 or <2026-01-01")),
            ("re:^img_\\d+   /^img_\\d+/", tx("A regular expression, whatever the case")),
        ];
        let ctx = ui.ctx().clone();
        let width = anchor.width().min(520.0);
        let height = 44.0 + LINES.len() as f32 * 36.0;
        let window = ctx.content_rect();
        let below = anchor.bottom() + 6.0 + height <= window.bottom();
        let top = if below { anchor.bottom() + 6.0 } else { (anchor.top() - 6.0 - height).max(window.top() + 4.0) };
        let rect = Rect::from_min_size(pos2(anchor.left(), top), vec2(width, height));
        Area::new(Id::unique("search-help-panel")).order(Order::Foreground).fixed_pos(rect.min).show(&ctx, |ui| {
            ui.set_min_size(rect.size());
            let round = pal.radius.saturating_add(2);
            ui.painter().add(ctx.global_style().visuals.popup_shadow.as_shape(rect, round));
            ui.painter().rect_filled(rect, round, mix(pal.bg, pal.surface, 0.6));
            ui.painter().rect_stroke(rect, round, Stroke::new(1.0, pal.line), StrokeKind::Inside);
            let mut y = rect.top() + 14.0;
            ui.painter().text(pos2(rect.left() + 14.0, y), Align2::LEFT_TOP, tr("What you can type"), semibold(13.0), pal.text);
            y += 26.0;
            for (example, what) in LINES {
                ui.painter().text(pos2(rect.left() + 14.0, y), Align2::LEFT_TOP, example, FontId::monospace(12.0), pal.accent);
                let galley = line_galley(ui, tr(what), FontId::proportional(12.0), pal.sub, width - 28.0);
                ui.painter().galley(pos2(rect.left() + 14.0, y + 16.0), galley, pal.sub);
                y += 36.0;
            }
        });
        let pressed_elsewhere = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p) && !anchor.contains(p)));
        if pressed_elsewhere || ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.help_open = false;
        }
    }

    fn timing_label(&self) -> String {
        if !self.cfg.show_timing || self.query.trim().is_empty() {
            return String::new();
        }
        let micros = self.file_micros.unwrap_or(0) + self.fast_micros;
        if micros < 1000 {
            crate::trf!("{n} µs", n = crate::i18n::fmt_count(micros as usize))
        } else {
            crate::trf!("{n} ms", n = crate::i18n::fmt_decimal(micros as f64 / 1000.0, 1))
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
                    section_title(ui, tr("All apps"), pal);
                    self.draw_app_list(ui, pal);
                }
            }
            ui.add_space(8.0);
        }
    }

    fn draw_welcome(&mut self, ui: &mut Ui, pal: Pal) {
        Frame::new().fill(pal.surface).corner_radius(pal.radius + 2).inner_margin(12).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(tr("This is your new Start menu")).font(semibold(14.0)).color(pal.text));
            ui.add_space(2.0);
            let text = if self.cfg.take_over_start {
                tr("The Start key and Start button open it now. Shift + Start still opens the Windows one. Pick another look any time in Settings.")
            } else {
                tr("Pick another look any time in Settings.")
            };
            ui.label(RichText::new(text).size(13.0).color(pal.sub));
            ui.add_space(6.0);
            if pill_button(ui, tr("Got it"), pal, true).clicked() {
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
            section_title(ui, if group.name == "Pinned" { tr("Pinned") } else { group.name.as_str() }, pal); // i18n-ignore: the first section's saved name
            if pal.style == Experience::Windows11 && group_index == 0 {
                let spot = Rect::from_min_size(pos2(ui.max_rect().right() - 70.0, title_top + 1.0), vec2(70.0, 24.0));
                let all = ui.interact(spot, Id::unique("win11-all"), Sense::click());
                ui.painter().rect_filled(spot, pal.radius, if all.hovered() { pal.hover } else { pal.surface });
                ui.painter().text(spot.center() - vec2(6.0, ink_nudge(12.5)), Align2::CENTER_CENTER, tr("All"), FontId::proportional(12.5), pal.text);
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
            ui.label(RichText::new(tr("Right-click any app or file to pin it here.")).size(13.0).color(pal.sub));
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
            line = crate::trf!("{hint} (may need a sign-out to show)", hint = line);
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
        section_title(ui, if pal.style == Experience::Windows11 { tr("Recommended") } else { tr("Recent files") }, pal);
        if list.is_empty() {
            ui.label(RichText::new(tr("Files you open will show up here.")).size(13.0).color(pal.sub));
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
        section_title(ui, tr("Most used"), pal);
        if items.is_empty() {
            ui.label(RichText::new(tr("What you open most will show up here.")).size(13.0).color(pal.sub));
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
                        detail: crate::trn!(usage.count, "Opened once", "Opened {n} times"),
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
            ui.label(RichText::new(tr("All apps")).font(semibold(14.0)).color(pal.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if pill_button(ui, tr("Back"), pal, false).clicked() {
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
            ui.label(RichText::new(tr("Reading your apps...")).size(13.0).color(pal.sub));
        }
    }

    /// How much room the file type buttons take, right beside the search bar.
    pub(crate) fn chip_row_height(&self) -> f32 {
        if self.cfg.show_file_buttons && !self.query.trim().is_empty() && (self.facets.total > 0 || self.filtering()) { 32.0 } else { 0.0 }
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
            chips.push((tr("Folders").to_string(), Some(facets.folders), filter.folders, Press::Folders));
        }
        let mut in_kinds: Vec<&str> = Vec::new();
        for (label, exts) in FILE_KINDS {
            let all_on = exts.iter().all(|e| filter.exts.iter().any(|on| on == e));
            let count: u32 = exts.iter().map(|e| count_of(e)).sum();
            if all_on {
                in_kinds.extend(exts.iter().copied());
            }
            if count > 0 || all_on {
                chips.push((tr(*label).to_string(), Some(count), all_on, Press::Kind(exts)));
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
        let clear_room = if filtering { chip_width(ui, tr("Clear"), None) + 6.0 } else { 0.0 };
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
        if filtering && chip(&mut child, tr("Clear"), None, false, pal).clicked() {
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
        if let Some(error) = &self.search_error {
            ui.add_space(6.0);
            ui.label(RichText::new(crate::trf!("This regular expression can't be used: {error}", error = error)).size(13.0).color(Color32::from_rgb(222, 120, 96)));
        }
        if rows.is_empty() {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(crate::trf!("Nothing found for \"{query}\"", query = self.query.trim())).size(14.0).color(pal.sub));
            });
        }
        let mut last = None;
        for (at, row) in rows.iter().enumerate() {
            if self.best_at == Some(at) {
                // The rows after it start their own section, even one of the same kind.
                last = None;
                section_title(ui, tr("Best match"), pal);
            } else if last != Some(row.category) {
                last = Some(row.category);
                section_title(ui, tr(row.category.label()), pal);
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
        let id = row.nav_id();
        let response = ui.interact(rect, id, Sense::click());
        let intent = match &row.kind {
            RowKind::Open(target) => Some(Intent::Open { name: row.name.clone(), target: target.clone(), admin: false }),
            RowKind::Run(what) => Some(Intent::Run(*what)),
            RowKind::Web(url) => Some(Intent::Web(url.clone())),
            RowKind::Person(person) => Some(Intent::Link(crate::m365::chat_link(person.teams_id(), self.teams_installed))),
            RowKind::Link(url) => Some(Intent::Link(url.clone())),
            RowKind::Copy(text) => Some(Intent::CopyAnswer(text.clone())),
            RowKind::Paste(text) => Some(Intent::Paste(text.clone())),
            RowKind::Output(id) => Some(Intent::UseOutput(id.clone())),
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
        // A person has buttons at the end of the row: chat and call in Teams, and mail.
        let mut text_right = rect.right() - 8.0;
        if let RowKind::Person(person) = &row.kind {
            let teams = self.teams_installed;
            let mut buttons = vec![('\u{E8BD}', tr("Chat in Teams"), crate::m365::chat_link(person.teams_id(), teams)), ('\u{E717}', tr("Call in Teams"), crate::m365::call_link(person.teams_id(), teams))];
            if !person.mail.is_empty() {
                buttons.push(('\u{E715}', tr("Mail"), crate::m365::mail_link(&person.mail)));
            }
            let size = if compact { 24.0 } else { 28.0 };
            let mut x = rect.right() - 6.0 - size * buttons.len() as f32;
            text_right = x - 4.0;
            for (glyph, tip, link) in buttons {
                let area = Rect::from_min_size(pos2(x, rect.center().y - size / 2.0), vec2(size, size));
                let pressed = ui.interact(area, id.with(glyph as u32), Sense::click()).on_hover_text(tip);
                if pressed.hovered() {
                    ui.painter().rect_filled(area, pal.radius.min(4), pal.hover);
                }
                ui.painter().text(area.center(), Align2::CENTER_CENTER, glyph, icon_font(13.0), sub_ink);
                if pressed.clicked() {
                    self.intents.push(Intent::Link(link));
                }
                x += size;
            }
        }
        let text_width = text_right - text_left;
        let confirming = matches!(row.kind, RowKind::Run(what) if self.confirm.is_some_and(|(d, at)| d == what && at.elapsed() < Duration::from_secs(5)));
        let shown_name = if confirming { crate::trf!("Press again to confirm: {name}", name = row.name) } else { row.name.clone() };
        let name_color = if confirming && !lit { pal.accent } else { name_ink };
        let mark = if lit && pal.style == Experience::WindowsXp { pal.on_accent } else { pal.accent };
        // Compact rows have no second line, so a file's folder follows its name: files of the same name stay told apart.
        let folder_after = compact && show_detail && (row.path.is_some() || matches!(row.kind, RowKind::Person(_) | RowKind::Link(_))) && !row.detail.is_empty();
        let gap = 10.0;
        let name_room = if folder_after {
            let folder_width = ui.painter().layout_no_wrap(row.detail.clone(), FontId::proportional(12.0), sub_ink).size().x;
            (text_width - gap - folder_width.min(text_width * 0.45)).max(text_width * 0.5)
        } else {
            text_width
        };
        let name = highlighted_galley(ui, &shown_name, &self.tokens, FontId::proportional(14.0), name_color, mark, name_room);
        if folder_after {
            let left = text_width - name.size().x - gap;
            if left >= 40.0 {
                let folder = path_galley(ui, &row.detail, FontId::proportional(12.0), sub_ink, left);
                let y = rect.center().y - folder.size().y / 2.0 - ink_nudge(12.0);
                ui.painter().galley(pos2(text_left + name.size().x + gap, y), folder, sub_ink);
            }
            ui.painter().galley(pos2(text_left, rect.center().y - name.size().y / 2.0 - ink_nudge(14.0)), name, name_ink);
        } else if two_lines {
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
            let mut entries = popup::target_entries(&self.cfg, &row.name, target, path.as_deref(), false);
            entries.extend(popup::bookmark_entries(&self.cfg, row, target, self.policy.added("/bookmarks")));
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
        let entries = vec![item('\u{E708}', tr("Sleep"), Do::Sleep), item('\u{E7E8}', tr("Shut down"), Do::ShutDown), item('\u{E777}', tr("Restart"), Do::Restart)];
        let height = popup::height_of(&entries);
        self.open_popup(pos2(anchor.right() - 248.0, anchor.top() - height - 4.0), entries);
    }

    fn draw_footer(&mut self, ui: &mut Ui, rect: Rect, pal: Pal) {
        // In the middle of the darker band in Windows 11, which reaches down to the window edge.
        let center = rect.center().y + if pal.style == Experience::Windows11 { 5.0 } else { 0.0 };
        if !self.query.trim().is_empty() {
            if !self.cfg.show_search_hints {
                return;
            }
            let on_setting = self.selected_entry().is_some_and(|e| matches!(e.kind, NavKind::Quick(..)));
            // What Enter would open: the selected result, or the first one when none is.
            let target = self.selected.or_else(|| self.nav.first().map(|n| n.id));
            let current = target.and_then(|id| self.rows.iter().find(|row| row.nav_id() == id)).map(|row| &row.kind);
            let hint = if on_setting {
                tr("Enter applies, Down, then left and right adjusts, Right-click for more, Esc clears")
            } else if matches!(current, Some(RowKind::Person(_))) {
                tr("Enter starts a Teams chat, Esc clears")
            } else if matches!(current, Some(RowKind::Link(_))) {
                tr("Enter opens the message, Esc clears")
            } else if matches!(current, Some(RowKind::Copy(_))) {
                tr("Enter copies the result, Esc clears")
            } else if matches!(current, Some(RowKind::Paste(_))) {
                tr("Enter pastes it where you were, Esc clears")
            } else if matches!(current, Some(RowKind::Output(_))) {
                tr("Enter plays sound there, Esc clears")
            } else if matches!(current, Some(RowKind::Open(_))) && self.rows.iter().find(|row| Some(row.nav_id()) == target).is_some_and(|row| row.category == Category::Bookmarks) {
                tr("Enter opens the bookmark, right-click for more, Esc clears")
            } else if matches!(current, Some(RowKind::Web(_) | RowKind::Run(_))) {
                tr("Enter opens, Esc clears")
            } else {
                match (self.policy.allows("Menu_RunAsAdmin"), self.policy.allows("Menu_OpenLocation")) {
                    (true, true) => tr("Enter opens, Ctrl+Enter as admin, Shift+Enter shows in folder, Esc clears"),
                    (true, false) => tr("Enter opens, Ctrl+Enter as admin, Esc clears"),
                    (false, true) => tr("Enter opens, Shift+Enter shows in folder, Esc clears"),
                    (false, false) => tr("Enter opens, Esc clears"),
                }
            };
            let galley = line_galley(ui, hint, FontId::proportional(12.0), pal.sub, rect.width());
            ui.painter().galley(pos2(rect.left() + 4.0, center - galley.size().y / 2.0 - ink_nudge(12.0)), galley, pal.sub);
            return;
        }
        // Your picture and name are one button, opening what can be done with your account.
        let status = self.status_text();
        let label = if status.is_empty() { platform::user_name() } else { status };
        let galley = line_galley(ui, &label, FontId::proportional(13.0), pal.text, rect.width() - 180.0);
        let button = Rect::from_min_size(pos2(rect.left(), center - 18.0), vec2(galley.size().x + TEXT_COLUMN + 12.0, 36.0));
        let user = ui.interact(button, Id::unique("user-button"), Sense::click()).on_hover_text(tr("Account"));
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
        let power = if can_power { Some(place(ui, "power", '\u{E7E8}', tr("Power"), false)) } else { None };
        let gear = if self.policy.allows_do(Do::Customize) { Some(place(ui, "customize", '\u{E713}', tr("Settings"), false)) } else { None };
        // Windows 11 keeps this as the "All" button above the pinned apps.
        if pal.style == Experience::Startline {
            let all = place(ui, "all", '\u{E71D}', tr("All apps"), self.page == Page::AllApps);
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
            return crate::trf!("Reading your folders... {count} so far", count = thousands(walked));
        }
        String::new()
    }
}

