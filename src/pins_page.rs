//! The Pinned apps page: your real icons to drag around, and three easy ways to add more.

use crate::config::{Pin, PinGroup, Target};
use crate::popup::Entry;
use crate::search::Icon;
use crate::ui::{Intent, Menu, Picked};
use crate::widgets::*;
use egui::*;

#[derive(Clone, Copy, PartialEq, Default)]
enum AddFrom {
    #[default]
    Apps,
    Files,
    Website,
}

#[derive(Default)]
pub struct PinUi {
    add_from: AddFrom,
    app_filter: String,
    file_path: String,
    website: String,
    selected: Option<(usize, usize)>,
    dragging: Option<(usize, usize)>,
    show_add: bool,
}

impl PinUi {
    /// For taking a picture of the page with one pin open.
    pub fn with_first_selected() -> PinUi {
        PinUi { selected: Some((0, 0)), ..Default::default() }
    }
}

/// "github.com" or "https://github.com/me" becomes a pin named after the site.
pub fn website_pin(text: &str) -> Option<Pin> {
    let text = text.trim();
    if text.is_empty() || text.contains(' ') || !text.contains('.') {
        return None;
    }
    let url = if text.contains("://") { text.to_string() } else { format!("https://{text}") };
    let host = url.split("://").nth(1)?.split(['/', '?', '#']).next()?;
    let name = host.strip_prefix("www.").unwrap_or(host);
    (!name.is_empty()).then(|| Pin { name: name.to_string(), target: Target::path(url.clone()) })
}

/// A file is named without its ending, a folder or drive by itself.
pub fn path_pin(path: &str) -> Pin {
    let path = path.trim().trim_matches('"');
    let last = path.trim_end_matches('\\').rsplit('\\').next().unwrap_or(path);
    let name = match last.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => last,
    };
    Pin { name: name.to_string(), target: Target::path(path) }
}

enum Act {
    Move { from: (usize, usize), to: (usize, usize) },
    MoveToGroup { from: (usize, usize), group: usize },
    Unpin((usize, usize)),
    Select((usize, usize)),
    ShowAdd,
}

pub fn draw(menu: &mut Menu, ui: &mut Ui, pal: Pal) {
    crate::customize::page_header(ui, "Pinned apps", "Drag to rearrange, click one to edit it, right-click for more.", pal);
    let policy = menu.policy.clone();
    policy.lock(ui, &["PinnedApps"], |ui| two_columns(menu, ui, |menu, ui| pin_grid(menu, ui, pal), |menu, ui| add_panel(menu, ui, pal)));
}

fn pin_grid(menu: &mut Menu, ui: &mut Ui, pal: Pal) {
    crate::customize::heading(ui, "On your Start menu", pal);
    if !ui.input(|i| i.pointer.any_down()) {
        menu.pin_ui.dragging = None;
    }
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let groups = menu.cfg.groups.clone();
    let mut act = None;
    let tile = vec2(92.0, 88.0);
    let columns = ((ui.available_width() / tile.x) as usize).max(3);
    for (g, group) in groups.iter().enumerate() {
        if groups.len() > 1 {
            group_heading(menu, ui, g, groups.len(), pal);
        }
        let count = group.pins.len() + usize::from(g == 0);
        let area_top = ui.cursor().min;
        if count == 0 {
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::hover());
            ui.painter().rect_stroke(rect, pal.radius + 2, Stroke::new(1.0, pal.line), StrokeKind::Inside);
            ui.painter().text(rect.center() - vec2(0.0, ink_nudge(12.5)), Align2::CENTER_CENTER, "Drag apps here", FontId::proportional(12.5), pal.sub);
            if let (Some(from), Some(point)) = (menu.pin_ui.dragging, pointer) {
                if rect.contains(point) {
                    act = Some(Act::MoveToGroup { from, group: g });
                }
            }
        }
        for row in 0..count.div_ceil(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                for column in 0..columns {
                    let at = row * columns + column;
                    if at < group.pins.len() {
                        pin_tile(menu, ui, tile, &group.pins[at], (g, at), pointer, &mut act, pal);
                    } else if at == group.pins.len() && g == 0 {
                        add_tile(ui, tile, &mut act, pal);
                    }
                }
            });
        }
        let _ = area_top;
        ui.add_space(4.0);
    }

    match act {
        Some(Act::Move { from, to }) => {
            let pin = menu.cfg.groups[from.0].pins.remove(from.1);
            let at = to.1.min(menu.cfg.groups[to.0].pins.len());
            menu.cfg.groups[to.0].pins.insert(at, pin);
            menu.pin_ui.dragging = Some((to.0, at));
            menu.pin_ui.selected = None;
        }
        Some(Act::MoveToGroup { from, group }) => {
            let pin = menu.cfg.groups[from.0].pins.remove(from.1);
            menu.cfg.groups[group].pins.push(pin);
            menu.pin_ui.dragging = Some((group, menu.cfg.groups[group].pins.len() - 1));
            menu.pin_ui.selected = None;
        }
        Some(Act::Unpin((g, p))) => {
            menu.cfg.groups[g].pins.remove(p);
            menu.pin_ui.selected = None;
        }
        Some(Act::Select(slot)) => {
            menu.pin_ui.selected = if menu.pin_ui.selected == Some(slot) { None } else { Some(slot) };
        }
        Some(Act::ShowAdd) => {
            menu.pin_ui.selected = None;
            menu.pin_ui.show_add = true;
        }
        None => {}
    }

    if let Some((g, p)) = menu.pin_ui.selected {
        if menu.cfg.groups.get(g).is_some_and(|group| p < group.pins.len()) {
            pin_details(menu, ui, g, p, pal);
        } else {
            menu.pin_ui.selected = None;
        }
    }

    ui.add_space(4.0);
    if pill_button(ui, "New section", pal, false).on_hover_text("Sections group your pins under a heading.").clicked() {
        let name = format!("Section {}", menu.cfg.groups.len() + 1);
        menu.cfg.groups.push(PinGroup { name, pins: Vec::new() });
    }
}

fn group_heading(menu: &mut Menu, ui: &mut Ui, g: usize, group_count: usize, pal: Pal) {
    let response = ui
        .horizontal(|ui| {
            let edit = TextEdit::singleline(&mut menu.cfg.groups[g].name).frame(Frame::NONE).font(semibold(14.0)).text_color(pal.text).desired_width(220.0);
            ui.add(edit).on_hover_text("Click to rename. Right-click for more.")
        })
        .inner;
    if response.secondary_clicked() {
        let at = response.interact_pointer_pos().unwrap_or(response.rect.left_bottom());
        let item = |glyph, label: &str, intent| Entry::Item { glyph, label: label.to_string(), intent };
        let mut entries = Vec::new();
        if g > 0 {
            entries.push(item('\u{E74A}', "Move section up", Intent::MoveSection(g, -1)));
        }
        if g + 1 < group_count {
            entries.push(item('\u{E74B}', "Move section down", Intent::MoveSection(g, 1)));
        }
        entries.push(item('\u{E74D}', "Remove section, keep its pins", Intent::RemoveSection(g)));
        menu.open_popup(at, entries);
    }
}

#[allow(clippy::too_many_arguments)]
fn pin_tile(menu: &mut Menu, ui: &mut Ui, size: Vec2, pin: &Pin, slot: (usize, usize), pointer: Option<Pos2>, act: &mut Option<Act>, pal: Pal) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let body = rect.shrink(3.0);
    let id = Id::unique(("pin-tile", slot.0, &pin.target));
    let response = ui.interact(body, id, Sense::click_and_drag()).on_hover_cursor(CursorIcon::Grab);
    if response.drag_started() {
        menu.pin_ui.dragging = Some(slot);
    }
    if let (Some(from), Some(point)) = (menu.pin_ui.dragging, pointer) {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        if from != slot && rect.contains(point) && act.is_none() {
            *act = Some(Act::Move { from, to: slot });
        }
    }
    let dragging = menu.pin_ui.dragging == Some(slot);
    let selected = menu.pin_ui.selected == Some(slot);
    if dragging || selected {
        ui.painter().rect_filled(body, pal.radius + 2, pal.selected);
    } else if response.hovered() {
        ui.painter().rect_filled(body, pal.radius + 2, pal.hover);
    }
    if selected {
        ui.painter().rect_stroke(body, pal.radius + 2, Stroke::new(1.5, pal.accent), StrokeKind::Inside);
    }
    let icon = Rect::from_center_size(pos2(rect.center().x, rect.top() + 32.0), Vec2::splat(36.0));
    menu.draw_icon(ui, icon, &Icon::for_target_cached(&pin.target), &pin.name, pal);
    let label = line_galley(ui, &pin.name, FontId::proportional(12.0), pal.text, rect.width() - 10.0);
    ui.painter().galley(pos2(rect.center().x - label.size().x / 2.0, rect.top() + 58.0), label, pal.text);

    let mut remove_clicked = false;
    if response.hovered() || selected {
        let corner = Rect::from_center_size(body.right_top() + vec2(-10.0, 10.0), Vec2::splat(18.0));
        let remove = ui.interact(corner, id.with("unpin"), Sense::click()).on_hover_text("Unpin");
        ui.painter().circle_filled(corner.center(), 9.0, if remove.hovered() { pal.accent } else { pal.surface });
        ui.painter().text(corner.center(), Align2::CENTER_CENTER, '\u{E711}', icon_font(9.0), if remove.hovered() { pal.on_accent } else { pal.text });
        if remove.clicked() {
            *act = Some(Act::Unpin(slot));
            remove_clicked = true;
        }
    }
    if response.clicked() && !remove_clicked && act.is_none() {
        *act = Some(Act::Select(slot));
    }
    if response.secondary_clicked() {
        let path = menu.path_of_target(&pin.target);
        let entries = crate::popup::target_entries(&menu.cfg, &pin.name, &pin.target, path.as_deref(), false);
        menu.open_popup(response.interact_pointer_pos().unwrap_or(rect.center()), entries);
    }
}

fn add_tile(ui: &mut Ui, size: Vec2, act: &mut Option<Act>, pal: Pal) {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let body = rect.shrink(3.0);
    let color = if response.hovered() { pal.text } else { pal.sub };
    if response.hovered() {
        ui.painter().rect_filled(body, pal.radius + 2, pal.hover);
    }
    ui.painter().rect_stroke(body, pal.radius + 2, Stroke::new(1.0, pal.line), StrokeKind::Inside);
    ui.painter().text(pos2(rect.center().x, rect.top() + 32.0), Align2::CENTER_CENTER, '\u{E710}', icon_font(20.0), color);
    ui.painter().text(pos2(rect.center().x, rect.top() + 64.0), Align2::CENTER_CENTER, "Add", FontId::proportional(12.0), color);
    if response.clicked() {
        *act = Some(Act::ShowAdd);
    }
}

fn pin_details(menu: &mut Menu, ui: &mut Ui, g: usize, p: usize, pal: Pal) {
    Frame::new().fill(pal.surface).stroke(Stroke::new(1.0, pal.line)).corner_radius(pal.radius + 2).inner_margin(12).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let pin = &mut menu.cfg.groups[g].pins[p];
        ui.horizontal(|ui| {
            ui.label(RichText::new("Name").size(13.0).color(pal.sub));
            ui.add(TextEdit::singleline(&mut pin.name).desired_width(280.0));
        });
        match &mut pin.target {
            Target::App(_) => {
                ui.label(RichText::new("An installed app").size(12.5).color(pal.sub));
            }
            Target::Path { path, args } => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Opens").size(13.0).color(pal.sub));
                    ui.add(TextEdit::singleline(path).desired_width(ui.available_width()));
                });
                let is_web = path.contains("://");
                if !is_web {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Options").size(13.0).color(pal.sub));
                        ui.add(TextEdit::singleline(args).desired_width(280.0).hint_text("Only if a program needs them"));
                    });
                }
            }
        }
    });
    ui.add_space(8.0);
}

fn add_panel(menu: &mut Menu, ui: &mut Ui, pal: Pal) {
    crate::customize::heading(ui, "Add to Start", pal);
    let panel = Frame::new().fill(pal.surface).stroke(Stroke::new(1.0, pal.line)).corner_radius(pal.radius + 2).inner_margin(12).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let options = [AddFrom::Apps, AddFrom::Files, AddFrom::Website];
        let now = options.iter().position(|o| *o == menu.pin_ui.add_from).unwrap_or(0);
        let width = ui.available_width().min(360.0);
        if let Some(i) = segmented(ui, "pin-add-from", &["App", "File or folder", "Website"], now, width, pal) {
            menu.pin_ui.add_from = options[i];
        }
        ui.add_space(8.0);
        match menu.pin_ui.add_from {
            AddFrom::Apps => add_app(menu, ui, pal),
            AddFrom::Files => add_file(menu, ui, pal),
            AddFrom::Website => add_website(menu, ui, pal),
        }
    });
    if std::mem::take(&mut menu.pin_ui.show_add) {
        ui.scroll_to_rect(panel.response.rect, Some(Align::BOTTOM));
    }
}

fn add_app(menu: &mut Menu, ui: &mut Ui, pal: Pal) {
    ui.add(TextEdit::singleline(&mut menu.pin_ui.app_filter).hint_text("Find an app").desired_width(f32::INFINITY));
    ui.add_space(4.0);
    let wanted = crate::matching::lower(menu.pin_ui.app_filter.trim());
    let apps = menu.apps.read().unwrap().clone();
    let matching: Vec<_> = apps.iter().filter(|a| wanted.is_empty() || a.lower.contains(&wanted)).take(300).collect();
    if matching.is_empty() {
        ui.label(RichText::new("No app by that name. Try \"A file or folder\" above.").size(12.5).color(pal.sub));
        return;
    }
    ScrollArea::vertical().id_salt("pin-apps").min_scrolled_height(400.0).max_height(400.0).auto_shrink([false, true]).show(ui, |ui| {
        ui.set_width(ui.available_width() - 8.0);
        for app in matching {
            let target = Target::App(app.id.clone());
            let pinned = menu.cfg.is_pinned(&target);
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
            if !ui.is_rect_visible(rect) {
                continue;
            }
            if response.hovered() {
                ui.painter().rect_filled(rect, pal.radius, pal.hover);
            }
            let icon = Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), Vec2::splat(26.0));
            menu.draw_icon(ui, icon, &Icon::for_target_cached(&target), &app.name, pal);
            let name = line_galley(ui, &app.name, FontId::proportional(13.5), pal.text, rect.width() - 150.0);
            ui.painter().galley(pos2(rect.left() + 42.0, rect.center().y - name.size().y / 2.0 - ink_nudge(13.5)), name, pal.text);
            let (label, color) = if pinned { ("Pinned", pal.sub) } else { ("Pin", pal.accent) };
            if pinned {
                ui.painter().text(pos2(rect.right() - 66.0, rect.center().y), Align2::RIGHT_CENTER, '\u{E73E}', icon_font(12.0), pal.sub);
            }
            ui.painter().text(pos2(rect.right() - 12.0, rect.center().y - ink_nudge(13.0)), Align2::RIGHT_CENTER, label, semibold(13.0), color);
            if response.on_hover_text(if pinned { "Click to unpin" } else { "Click to pin" }).clicked() {
                menu.cfg.toggle_pin(&app.name, &target);
            }
        }
    });
}

fn add_file(menu: &mut Menu, ui: &mut Ui, pal: Pal) {
    ui.horizontal(|ui| {
        if pill_button(ui, "Choose a file...", pal, true).clicked() {
            menu.ask_for_path(ui.ctx(), false, Picked::Pin);
        }
        if pill_button(ui, "Choose a folder...", pal, true).clicked() {
            menu.ask_for_path(ui.ctx(), true, Picked::Pin);
        }
    });
    ui.add_space(6.0);
    ui.label(RichText::new("Or paste where it is, like a program, a folder or a network location:").size(12.5).color(pal.sub));
    let mut add = false;
    ui.horizontal(|ui| {
        let width = ui.available_width() - 70.0;
        let field = ui.add(TextEdit::singleline(&mut menu.pin_ui.file_path).desired_width(width).hint_text("C:\\Tools\\app.exe"));
        add = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        add |= pill_button(ui, "Add", pal, false).clicked();
    });
    let path = menu.pin_ui.file_path.trim().to_string();
    if add && !path.is_empty() {
        let pin = path_pin(&path);
        if let Some(first) = menu.cfg.groups.first_mut() {
            first.pins.push(pin);
        }
        menu.pin_ui.file_path.clear();
    }
}

fn add_website(menu: &mut Menu, ui: &mut Ui, pal: Pal) {
    let mut add = false;
    ui.horizontal(|ui| {
        let width = ui.available_width() - 70.0;
        let field = ui.add(TextEdit::singleline(&mut menu.pin_ui.website).desired_width(width).hint_text("example.com"));
        add = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        add |= pill_button(ui, "Add", pal, false).clicked();
    });
    if add {
        if let Some(pin) = website_pin(&menu.pin_ui.website) {
            if let Some(first) = menu.cfg.groups.first_mut() {
                first.pins.push(pin);
            }
            menu.pin_ui.website.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_web_address_becomes_a_named_pin() {
        let pin = website_pin("https://www.example.com/docs?x=1").unwrap();
        assert_eq!(pin.name, "example.com");
        assert_eq!(pin.target, Target::path("https://www.example.com/docs?x=1"));
        let bare = website_pin(" github.com ").unwrap();
        assert_eq!(bare.name, "github.com");
        assert_eq!(bare.target, Target::path("https://github.com"));
        assert!(website_pin("not a site").is_none());
        assert!(website_pin("word").is_none());
    }

    #[test]
    fn a_path_is_named_by_its_last_part() {
        assert_eq!(path_pin("C:\\Tools\\My App.exe").name, "My App");
        assert_eq!(path_pin("\"C:\\Users\\me\\Documents\\\"").name, "Documents");
        assert_eq!(path_pin("D:\\").name, "D:");
        assert_eq!(path_pin("\\\\server\\share").name, "share");
        assert_eq!(path_pin("C:\\a\\.config").name, ".config");
    }
}
