//! The Start menu layouts of the older looks. Pins, results and lists are the same pieces
//! as everywhere else; only how they are arranged and framed differs.

use crate::actions::Do;
use crate::customize::Tab;
use crate::platform;
use crate::search::{Icon, Row, RowKind};
use crate::config::Category;
use crate::ui::{Intent, Menu, NavEntry, NavKind, Page};
use crate::widgets::*;
use egui::*;

const DOCUMENTS: Do = Do::Open("shell:Personal");
const PICTURES: Do = Do::Open("shell:My Pictures");
const MUSIC: Do = Do::Open("shell:My Music");
const COMPUTER: Do = Do::Open("shell:MyComputerFolder");
const CONTROL_PANEL: Do = Do::Open("control");
const DEVICES: Do = Do::Open("ms-settings:printers");
const DEFAULT_APPS: Do = Do::Open("ms-settings:defaultapps");

impl Menu {
    fn searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    /// A scrolling part of a layout.
    pub(crate) fn scroll_in(&mut self, ui: &mut Ui, rect: Rect, salt: &str, draw: impl FnOnce(&mut Menu, &mut Ui)) {
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)));
        child.set_clip_rect(rect.expand(1.0));
        ScrollArea::vertical().id_salt(salt).auto_shrink([false, false]).show(&mut child, |ui| {
            ui.set_width(ui.available_width());
            draw(self, ui);
        });
    }

    fn pin_rows(&mut self, ui: &mut Ui, pal: Pal) {
        let width = ui.available_width();
        for group in self.cfg.groups.clone() {
            for pin in group.pins {
                let row = Row {
                    category: Category::Apps,
                    name: pin.name.clone(),
                    detail: String::new(),
                    icon: Icon::for_target_cached(&pin.target),
                    kind: RowKind::Open(pin.target),
                    path: None,
                };
                self.draw_row(ui, &row, width, pal, false);
            }
        }
    }

    /// A line of text that opens something, as in the right column of the older menus.
    #[allow(clippy::too_many_arguments)]
    fn link(&mut self, ui: &Ui, rect: Rect, label: &str, what: Do, color: Color32, hot_fill: Color32, hot_color: Color32, bold: bool) {
        let id = Id::unique(("link", label));
        let response = ui.interact(rect, id, Sense::click());
        let intent = Intent::Run(what);
        self.nav_next.push(NavEntry { id, rect, kind: NavKind::Intent(intent.clone()) });
        let hot = response.hovered() || self.selected == Some(id);
        if hot {
            ui.painter().rect_filled(rect, 2, hot_fill);
        }
        let font = if bold { semibold(13.0) } else { FontId::proportional(13.0) };
        let text = if hot { hot_color } else { color };
        let galley = line_galley(ui, label, font, text, rect.width() - 16.0);
        ui.painter().galley(pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0 - ink_nudge(13.0)), galley, text);
        if response.clicked() {
            self.intents.push(intent);
        }
    }

    /// For layouts with search at the bottom: the file type buttons take the lowest strip,
    /// right above the search bar, and the results get the rest.
    fn split_for_chips(&self, area: Rect, gap: f32) -> (Rect, Rect) {
        let height = self.chip_row_height();
        if height == 0.0 {
            return (area, Rect::from_min_size(area.left_bottom(), Vec2::ZERO));
        }
        let chips = Rect::from_min_max(pos2(area.left(), area.bottom() - height), area.max);
        (Rect::from_min_max(area.min, pos2(area.right(), chips.top() - gap)), chips)
    }

    fn toggle_all_programs(&mut self) {
        let next = if self.page == Page::AllApps { Page::Home } else { Page::AllApps };
        self.intents.push(Intent::Show(next));
    }

    // ---- Windows 10: icon rail, app list and tiles ----

    pub(crate) fn draw_win10(&mut self, ui: &mut Ui, full: Rect, pal: Pal) {
        let rail = Rect::from_min_max(full.min, pos2(full.left() + 48.0, full.bottom()));
        let search = Rect::from_min_max(pos2(rail.right(), full.bottom() - 42.0), full.max);
        let body = Rect::from_min_max(pos2(rail.right(), full.top()), pos2(full.right(), search.top()));
        ui.painter().rect_filled(rail, 0, mix(pal.bg, pal.text, 0.05));

        self.draw_search_bar(ui, search.shrink2(vec2(8.0, 4.0)), pal);
        let inner = body.shrink(8.0);
        if self.searching() {
            let (list, chips) = self.split_for_chips(inner, 4.0);
            self.draw_filter_chips(ui, chips, pal);
            self.scroll_in(ui, list, "results", |menu, ui| menu.draw_results(ui, pal));
        } else {
            let list = Rect::from_min_max(inner.min, pos2(inner.left() + 270.0, inner.bottom()));
            let tiles = Rect::from_min_max(pos2(list.right() + 10.0, inner.top()), inner.max);
            self.scroll_in(ui, list, "apps", |menu, ui| menu.draw_app_list(ui, pal));
            self.scroll_in(ui, tiles, "tiles", |menu, ui| menu.draw_pinned(ui, pal));
        }

        let spot = |y: f32| Rect::from_center_size(pos2(rail.center().x, y), Vec2::splat(48.0));
        if icon_button(ui, spot(full.top() + 24.0), Id::unique("win10-all"), '\u{E700}', "All apps", self.page == Page::AllApps, pal).clicked() {
            self.toggle_all_programs();
        }
        let mut y = full.bottom() - 24.0;
        let power = icon_button(ui, spot(y), Id::unique("power"), '\u{E7E8}', "Power", false, pal);
        y -= 48.0;
        let gear = icon_button(ui, spot(y), Id::unique("customize"), '\u{E713}', "Settings", false, pal);
        y -= 48.0;
        let pictures = icon_button(ui, spot(y), Id::unique("win10-pictures"), '\u{E8B9}', "Pictures", false, pal);
        y -= 48.0;
        let documents = icon_button(ui, spot(y), Id::unique("win10-documents"), '\u{E8A5}', "Documents", false, pal);
        y -= 48.0;
        let user = ui.interact(spot(y), Id::unique("win10-user"), Sense::click()).on_hover_text(platform::user_name());
        if user.hovered() {
            ui.painter().rect_filled(user.rect, pal.radius, pal.hover);
        }
        self.draw_avatar(ui, Rect::from_center_size(user.rect.center(), Vec2::splat(26.0)), pal);
        if power.clicked() {
            self.open_power_menu(power.rect);
        }
        if gear.clicked() {
            self.intents.push(Intent::Show(Page::Customize(Tab::Appearance)));
        }
        if pictures.clicked() {
            self.intents.push(Intent::Run(PICTURES));
        }
        if documents.clicked() {
            self.intents.push(Intent::Run(DOCUMENTS));
        }
        if user.clicked() {
            self.open_user_menu(user.rect);
        }
    }

    // ---- Windows 7: white list on the left, glass all around ----

    pub(crate) fn draw_win7(&mut self, ui: &mut Ui, full: Rect, pal: Pal) {
        let (glass_top, glass_bottom) = (Color32::from_rgba_unmultiplied(28, 66, 104, 205), Color32::from_rgba_unmultiplied(92, 142, 176, 195));
        vertical_gradient(ui, full, glass_top, glass_bottom);
        ui.painter().rect_stroke(full.shrink(0.5), 6, Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 110)), StrokeKind::Inside);

        let content = full.shrink(8.0);
        let search_row = Rect::from_min_max(pos2(content.left(), content.bottom() - 34.0), content.max);
        let wide = self.searching();
        let pane_right = if wide { content.right() } else { content.left() + content.width() * 0.62 };
        let pane = Rect::from_min_max(content.min, pos2(pane_right, search_row.top() - 8.0));
        let search = Rect::from_min_max(search_row.min, pos2(pane_right, search_row.bottom()));

        ui.painter().rect_filled(pane, 3, Color32::WHITE);
        ui.painter().rect_stroke(pane, 3, Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 30, 60, 150)), StrokeKind::Outside);
        self.draw_search_bar(ui, search, pal);

        let inner = pane.shrink(5.0);
        if wide {
            let (list, chips) = self.split_for_chips(inner, 4.0);
            self.draw_filter_chips(ui, chips, pal);
            self.scroll_in(ui, list, "results", |menu, ui| menu.draw_results(ui, pal));
            return;
        }
        let all = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 30.0), inner.max);
        let list = Rect::from_min_max(inner.min, pos2(inner.right(), all.top() - 6.0));
        let showing_all = self.page == Page::AllApps;
        self.scroll_in(ui, list, "left", |menu, ui| if showing_all { menu.draw_app_list(ui, pal) } else { menu.pin_rows(ui, pal) });
        ui.painter().hline(inner.x_range(), all.top() - 3.0, Stroke::new(1.0, Color32::from_gray(214)));
        self.all_programs_row(ui, all, showing_all, Color32::BLACK, pal);

        let column = Rect::from_min_max(pos2(pane.right() + 8.0, content.top()), content.max);
        let picture = Rect::from_center_size(pos2(column.center().x, column.top() + 34.0), Vec2::splat(56.0));
        ui.painter().rect_filled(picture, 4, Color32::WHITE);
        self.draw_account_square(ui, picture.shrink(3.0), pal);
        let name = platform::user_name();
        let picture_click = ui.interact(picture, Id::unique("user-picture"), Sense::click()).on_hover_text(name.clone());
        if picture_click.clicked() {
            self.open_user_menu(picture);
        }

        let hot = Color32::from_rgba_unmultiplied(255, 255, 255, 50);
        let links = [
            ("Documents", DOCUMENTS),
            ("Pictures", PICTURES),
            ("Music", MUSIC),
            ("Computer", COMPUTER),
            ("Control Panel", CONTROL_PANEL),
            ("Devices and Printers", DEVICES),
            ("Default Programs", DEFAULT_APPS),
            ("Startline settings", Do::Customize),
        ];
        let policy = self.policy.clone();
        let mut y = picture.bottom() + 10.0;
        for (label, what) in links.into_iter().filter(|(_, what)| policy.allows_do(*what)) {
            let rect = Rect::from_min_size(pos2(column.left(), y), vec2(column.width(), 29.0));
            self.link(ui, rect, label, what, Color32::WHITE, hot, Color32::WHITE, false);
            y += 31.0;
        }

        let shut = Rect::from_min_max(pos2(column.left() + 16.0, search_row.top() + 3.0), pos2(column.right(), search_row.bottom() - 3.0));
        let response = ui.interact(shut, Id::unique("power"), Sense::click());
        let fill = if response.hovered() { Color32::from_rgba_unmultiplied(255, 255, 255, 70) } else { Color32::from_rgba_unmultiplied(255, 255, 255, 34) };
        ui.painter().rect_filled(shut, 3, fill);
        ui.painter().rect_stroke(shut, 3, Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 120)), StrokeKind::Inside);
        ui.painter().text(shut.center() - vec2(0.0, ink_nudge(13.0)), Align2::CENTER_CENTER, "Shut down", FontId::proportional(13.0), Color32::WHITE);
        if response.clicked() {
            self.open_power_menu(shut);
        }
    }

    /// The account picture as a square, the way the older menus framed it.
    fn draw_account_square(&mut self, ui: &Ui, rect: Rect, pal: Pal) {
        let path = self.user_picture.lock().unwrap().clone();
        match path.and_then(|path| self.icons.get("picture:account", &path)) {
            Some(id) => {
                Image::from_texture(load::SizedTexture::new(id, rect.size())).corner_radius(2).paint_at(ui, rect);
            }
            None => {
                ui.painter().rect_filled(rect, 2, mix(pal.accent, Color32::WHITE, 0.25));
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, first_letter(&platform::user_name()), semibold(rect.height() * 0.5), Color32::WHITE);
            }
        }
    }

    fn all_programs_row(&mut self, ui: &Ui, rect: Rect, showing_all: bool, color: Color32, pal: Pal) {
        let response = ui.interact(rect, Id::unique("all-programs"), Sense::click());
        let hot = response.hovered();
        if hot {
            paint_selection(ui, rect, pal);
        }
        let text = if hot && pal.style == crate::styles::Experience::WindowsXp { pal.on_accent } else { color };
        let label = if showing_all { "Back" } else { "All Programs" };
        ui.painter().text(pos2(rect.left() + 10.0, rect.center().y - ink_nudge(13.0)), Align2::LEFT_CENTER, label, semibold(13.0), text);
        let glyph = if showing_all { '\u{E76B}' } else { '\u{E76C}' };
        ui.painter().text(pos2(rect.right() - 16.0, rect.center().y), Align2::CENTER_CENTER, glyph, icon_font(12.0), text);
        if response.clicked() {
            self.toggle_all_programs();
        }
    }

    // ---- Windows XP: blue header and footer around two columns ----

    pub(crate) fn draw_xp(&mut self, ui: &mut Ui, full: Rect, pal: Pal) {
        let header = Rect::from_min_max(full.min, pos2(full.right(), full.top() + 62.0));
        let footer = Rect::from_min_max(pos2(full.left(), full.bottom() - 42.0), full.max);
        let body = Rect::from_min_max(pos2(full.left(), header.bottom() + 2.0), pos2(full.right(), footer.top()));

        vertical_gradient(ui, header, Color32::from_rgb(24, 104, 216), Color32::from_rgb(58, 136, 232));
        ui.painter().hline(full.x_range(), header.bottom() + 1.0, Stroke::new(2.0, Color32::from_rgb(232, 146, 48)));
        vertical_gradient(ui, footer, Color32::from_rgb(58, 136, 232), Color32::from_rgb(20, 92, 200));

        let name = platform::user_name();
        let picture = Rect::from_center_size(pos2(header.left() + 36.0, header.center().y), Vec2::splat(46.0));
        ui.painter().rect_filled(picture, 3, Color32::WHITE);
        self.draw_account_square(ui, picture.shrink(2.0), pal);
        ui.painter().text(pos2(picture.right() + 10.0, header.center().y - ink_nudge(16.0)), Align2::LEFT_CENTER, &name, semibold(16.0), Color32::WHITE);
        if ui.interact(header, Id::unique("user-header"), Sense::click()).clicked() {
            self.open_user_menu(Rect::from_min_size(picture.left_bottom() + vec2(0.0, 300.0), picture.size()));
        }

        let wide = self.searching();
        let split = if wide { body.right() } else { body.left() + body.width() * 0.56 };
        let left = Rect::from_min_max(body.min, pos2(split, body.bottom()));
        ui.painter().rect_filled(left, 0, Color32::WHITE);
        let search = Rect::from_min_max(pos2(left.left() + 6.0, left.bottom() - 32.0), pos2(left.right() - 6.0, left.bottom() - 6.0));
        self.draw_search_bar(ui, search, pal);

        let above = Rect::from_min_max(left.min + vec2(4.0, 6.0), pos2(left.right() - 4.0, search.top() - 4.0));
        if wide {
            let (list, chips) = self.split_for_chips(above, 4.0);
            self.draw_filter_chips(ui, chips, pal);
            self.scroll_in(ui, list, "results", |menu, ui| menu.draw_results(ui, pal));
        } else {
            let right = Rect::from_min_max(pos2(split, body.top()), body.max);
            ui.painter().rect_filled(right, 0, Color32::from_rgb(211, 229, 250));
            ui.painter().vline(split, body.y_range(), Stroke::new(1.0, Color32::from_rgb(149, 189, 240)));

            let all = Rect::from_min_max(pos2(above.left(), above.bottom() - 28.0), above.max);
            let list = Rect::from_min_max(above.min, pos2(above.right(), all.top() - 6.0));
            let showing_all = self.page == Page::AllApps;
            self.scroll_in(ui, list, "left", |menu, ui| if showing_all { menu.draw_app_list(ui, pal) } else { menu.pin_rows(ui, pal) });
            ui.painter().hline(above.x_range(), all.top() - 3.0, Stroke::new(1.0, Color32::from_rgb(214, 214, 214)));
            self.all_programs_row(ui, all, showing_all, Color32::BLACK, pal);

            let ink = Color32::from_rgb(0, 42, 118);
            let links = [
                ("My Documents", DOCUMENTS, true),
                ("My Pictures", PICTURES, true),
                ("My Music", MUSIC, true),
                ("My Computer", COMPUTER, true),
                ("Control Panel", CONTROL_PANEL, false),
                ("Printers and Faxes", DEVICES, false),
                ("Default Programs", DEFAULT_APPS, false),
                ("Startline settings", Do::Customize, false),
            ];
            let policy = self.policy.clone();
            let mut y = right.top() + 8.0;
            for (label, what, bold) in links.into_iter().filter(|(_, what, _)| policy.allows_do(*what)) {
                let rect = Rect::from_min_size(pos2(right.left() + 4.0, y), vec2(right.width() - 8.0, 28.0));
                self.link(ui, rect, label, what, ink, pal.accent, pal.on_accent, bold);
                y += 30.0;
            }
        }

        let center = footer.center().y;
        let off = self.xp_footer_button(ui, pos2(footer.right() - 112.0, center), "Turn Off", '\u{E7E8}', Color32::from_rgb(214, 74, 40));
        let log = self.xp_footer_button(ui, pos2(footer.right() - 212.0, center), "Log Off", '\u{E72E}', Color32::from_rgb(232, 158, 40));
        if log.clicked() {
            self.open_user_menu(log.rect);
        }
        if off.clicked() {
            self.open_power_menu(off.rect);
        }
    }

    fn xp_footer_button(&mut self, ui: &Ui, left_center: Pos2, label: &str, glyph: char, color: Color32) -> Response {
        let rect = Rect::from_min_size(left_center - vec2(0.0, 14.0), vec2(96.0, 28.0));
        let response = ui.interact(rect, Id::unique(("xp-footer", label)), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(rect, 3, Color32::from_rgba_unmultiplied(255, 255, 255, 46));
        }
        let square = Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), Vec2::splat(20.0));
        ui.painter().rect_filled(square, 3, color);
        ui.painter().text(square.center(), Align2::CENTER_CENTER, glyph, icon_font(12.0), Color32::WHITE);
        ui.painter().text(pos2(square.right() + 6.0, rect.center().y - ink_nudge(12.5)), Align2::LEFT_CENTER, label, FontId::proportional(12.5), Color32::WHITE);
        response
    }
}

fn first_letter(name: &str) -> String {
    name.chars().next().unwrap_or('?').to_uppercase().to_string()
}
