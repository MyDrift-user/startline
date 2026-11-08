//! The Search settings page: tick the places to search in a folder tree, flip simple skip
//! rules, and see the result at once. Changes re-read the folders by themselves.

use crate::config::Category;
use crate::customize::{card, heading};
use crate::platform::{self, Drive, DriveKind};
use crate::scope::{self, SKIP_PRESETS, Scope, Tick};
use crate::search::Icon;
use crate::ui::{Menu, Picked};
use crate::widgets::*;
use egui::*;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct ScopeUi {
    expanded: HashSet<String>,
    requested: HashSet<String>,
    children: Arc<Mutex<HashMap<String, Vec<(String, bool)>>>>,
    hidden: Arc<Mutex<HashSet<String>>>,
    drives: Arc<Mutex<Vec<Drive>>>,
    drives_read: Option<Instant>,
    counts: Vec<(String, usize)>,
    counts_read: Option<Instant>,
    toggle: Option<String>,
    new_name: String,
    new_ext: String,
    new_place: String,
}

impl ScopeUi {
    /// For taking a picture of the page with the home folder open.
    pub fn open_home(&mut self, ctx: &Context) {
        if let Some(home) = home_folder() {
            self.expanded.insert(home.clone());
            self.request_children(&home, ctx);
        }
    }

    /// Children of a folder, read on a helper thread the first time they're asked for.
    fn request_children(&mut self, path: &str, ctx: &Context) {
        if !self.requested.insert(path.to_string()) {
            return;
        }
        let (slot, hidden, path, ctx) = (self.children.clone(), self.hidden.clone(), path.to_string(), ctx.clone());
        std::thread::spawn(move || {
            let found = platform::child_folders(&path);
            let mut known = hidden.lock().unwrap();
            for (name, is_hidden) in &found {
                if *is_hidden {
                    known.insert(crate::matching::lower(&format!("{}\\{name}", path.trim_end_matches('\\'))));
                }
            }
            drop(known);
            slot.lock().unwrap().insert(path, found);
            ctx.request_repaint();
        });
    }

    fn refresh(&mut self, menu_index: &crate::index::IndexManager, ctx: &Context) {
        if self.drives_read.is_none_or(|at| at.elapsed() > Duration::from_secs(5)) {
            self.drives_read = Some(Instant::now());
            let (slot, ctx) = (self.drives.clone(), ctx.clone());
            std::thread::spawn(move || {
                *slot.lock().unwrap() = platform::describe_drives();
                ctx.request_repaint();
            });
        }
        if self.counts_read.is_none_or(|at| at.elapsed() > Duration::from_secs(1)) {
            self.counts_read = Some(Instant::now());
            if let Ok(index) = menu_index.index.try_read() {
                self.counts = index.counts_by_root();
            }
            ctx.request_repaint_after(Duration::from_secs(1));
        }
    }
}

/// Drives that are on without being listed in the settings.
pub fn auto_roots(auto_network: bool, auto_removable: bool) -> Vec<String> {
    platform::attached_drives()
        .into_iter()
        .filter(|(_, kind)| (auto_network && *kind == DriveKind::Network) || (auto_removable && *kind == DriveKind::Removable))
        .map(|(root, _)| root)
        .collect()
}

fn home_folder() -> Option<String> {
    std::env::var("USERPROFILE").ok()
}

/// Everything a row needs to know to answer "is this searched?" this frame.
struct View {
    all_roots: Vec<String>,
    excluded_paths: Vec<String>,
    excluded_names: Vec<String>,
    include_hidden: bool,
    hidden: HashSet<String>,
    children: HashMap<String, Vec<(String, bool)>>,
    counts: Vec<(String, usize)>,
}

impl View {
    fn with_scope<R>(&self, read: impl FnOnce(&Scope) -> R) -> R {
        let is_hidden = |path: &str| self.hidden.contains(&crate::matching::lower(path.trim_end_matches('\\')));
        read(&Scope {
            roots: &self.all_roots,
            excluded_paths: &self.excluded_paths,
            excluded_names: &self.excluded_names,
            include_hidden: self.include_hidden,
            is_hidden: &is_hidden,
        })
    }
}

fn switch_line(ui: &mut Ui, title: &str, about: &str, value: &mut bool, pal: Pal) {
    let width = ui.available_width();
    ui.horizontal(|ui| {
        ui.set_min_height(34.0);
        if about.is_empty() {
            ui.label(RichText::new(title).size(13.5).color(pal.text));
        } else {
            ui.vertical(|ui| {
                ui.set_max_width(width - 70.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(title).size(13.5).color(pal.text));
                ui.label(RichText::new(about).size(12.0).color(pal.sub));
            });
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| switch(ui, value, pal));
    });
}

const FOLDER: char = '\u{E8B7}';

#[allow(clippy::too_many_arguments)]
fn tree_row(menu: &mut Menu, ui: &mut Ui, view: &View, path: &str, label: &str, detail: &str, glyph: char, depth: usize, pal: Pal) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let id = Id::unique(("tree", path));
    let left = rect.left() + depth as f32 * 22.0;
    let row = ui.interact(rect, id, Sense::click());
    if row.hovered() {
        ui.painter().rect_filled(rect, pal.radius, pal.hover);
    }
    let center = rect.center().y;
    let open = menu.scope_ui.expanded.contains(path);
    let known = view.children.get(path);
    let has_children = known.is_none_or(|c| !c.is_empty());
    if has_children {
        let arrow = if open { '\u{E70D}' } else { '\u{E76C}' };
        ui.painter().text(pos2(left + 12.0, center), Align2::CENTER_CENTER, arrow, icon_font(10.0), pal.sub);
    }
    let (tick, blocked) = view.with_scope(|scope| (scope.tick(path), scope.rule_blocks(path) && !view.all_roots.iter().any(|r| scope::same_path(r, path))));
    let tick_rect = Rect::from_center_size(pos2(left + 34.0, center), vec2(26.0, 26.0));
    let tick_response = ui.interact(tick_rect, id.with("tick"), Sense::click()).on_hover_text(match tick {
        Tick::On => "Searched. Click to leave it out.",
        Tick::Off => "Not searched. Click to include it.",
        Tick::Some => "Partly searched. Click to change the whole folder.",
    });
    draw_tick(ui, tick_rect, tick, pal);
    if glyph == FOLDER {
        // Windows' own folder picture; they all share one, so it only loads once.
        let spot = Rect::from_center_size(pos2(left + 58.0, center), Vec2::splat(18.0));
        menu.draw_icon(ui, spot, &Icon::Image { key: "folder".into(), source: path.to_string() }, label, pal);
    } else {
        ui.painter().text(pos2(left + 58.0, center), Align2::CENTER_CENTER, glyph, icon_font(15.0), pal.text);
    }

    let right_text = if !detail.is_empty() {
        detail.to_string()
    } else if blocked && tick == Tick::Off {
        "Skipped by a rule".to_string()
    } else if let Some((_, count)) = view.counts.iter().find(|(root, _)| scope::same_path(root, path)) {
        format!("{} items", thousands(*count))
    } else {
        String::new()
    };
    let right = line_galley(ui, &right_text, FontId::proportional(12.0), pal.sub, (rect.width() * 0.4).max(60.0));
    let right_width = right.size().x;
    ui.painter().galley(pos2(rect.right() - right_width - 8.0, center - right.size().y / 2.0 - ink_nudge(12.0)), right, pal.sub);
    let color = if tick == Tick::Off { pal.sub } else { pal.text };
    let name_room = (rect.right() - right_width - 20.0 - (left + 72.0)).max(40.0);
    let name = line_galley(ui, label, FontId::proportional(13.5), color, name_room);
    ui.painter().galley(pos2(left + 72.0, center - name.size().y / 2.0 - ink_nudge(13.5)), name, color);

    if tick_response.clicked() {
        menu.scope_ui.toggle = Some(path.to_string());
    } else if row.clicked() && has_children {
        if open {
            menu.scope_ui.expanded.remove(path);
        } else {
            menu.scope_ui.expanded.insert(path.to_string());
            menu.scope_ui.request_children(path, ui.ctx());
        }
    }
    if open {
        match known {
            None => {
                ui.label(RichText::new("    Reading...").size(12.0).color(pal.sub));
            }
            Some(children) => {
                for (child, _) in children {
                    let child_path = format!("{}\\{child}", path.trim_end_matches('\\'));
                    tree_row(menu, ui, view, &child_path, child, "", FOLDER, depth + 1, pal);
                }
            }
        }
    }
}

fn drive_title(drive: &Drive) -> String {
    let letter = drive.root.trim_end_matches('\\');
    let kind = match drive.kind {
        DriveKind::Fixed => "Local drive",
        DriveKind::Network => "Network drive",
        DriveKind::Removable => "USB drive",
    };
    if drive.label.is_empty() { format!("{kind} ({letter})") } else { format!("{} ({letter})", drive.label) }
}

fn tag_list(ui: &mut Ui, list: &mut Vec<String>, pal: Pal) {
    // An empty row would leave a gap between the label and its box.
    if list.iter().all(|item| scope::in_a_preset(item)) {
        return;
    }
    let mut remove = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for (index, item) in list.iter().enumerate() {
            if scope::in_a_preset(item) {
                continue;
            }
            if chip(ui, &format!("{item}   ×"), None, false, pal).on_hover_text("Click to remove this rule").clicked() {
                remove = Some(index);
            }
        }
    });
    if let Some(index) = remove {
        list.remove(index);
    }
}

fn add_rule_field(ui: &mut Ui, text: &mut String, hint: &str, pal: Pal) -> bool {
    let mut add = false;
    ui.horizontal(|ui| {
        // Built from the right, so the button ends on the card's edge and the box fills the rest.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            add = pill_button(ui, "Add", pal, false).clicked();
            let field = ui.add(TextEdit::singleline(text).desired_width(f32::INFINITY).hint_text(hint));
            add |= field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        });
    });
    add && !text.trim().is_empty()
}

pub fn draw(menu: &mut Menu, ui: &mut Ui, pal: Pal) {
    menu.scope_ui.refresh(&menu.index, ui.ctx());
    let ctx = ui.ctx().clone();

    crate::customize::page_header(ui, "Search", "Where search looks, what it leaves out and how results are shown.", pal);

    // How things stand right now.
    let status = menu.index.status.clone();
    let line = if status.walking.load(Ordering::Relaxed) {
        ctx.request_repaint_after(Duration::from_millis(300));
        format!("Reading your folders... {} so far", thousands(status.walked.load(Ordering::Relaxed)))
    } else {
        format!("{} files and folders ready to search", thousands(menu.file_count()))
    };
    ui.label(RichText::new(line).size(13.0).color(pal.text));
    ui.add_space(8.0);

    let drives = menu.scope_ui.drives.lock().unwrap().clone();
    let autos = auto_roots(menu.cfg.auto_network, menu.cfg.auto_removable);
    let view = View {
        all_roots: scope::all_roots(&menu.cfg, &autos),
        excluded_paths: menu.cfg.excluded_paths.clone(),
        excluded_names: menu.cfg.exclude.clone(),
        include_hidden: menu.cfg.index_hidden,
        hidden: menu.scope_ui.hidden.lock().unwrap().clone(),
        children: menu.scope_ui.children.lock().unwrap().clone(),
        counts: menu.scope_ui.counts.clone(),
    };

    let home = home_folder();
    let policy = menu.policy.clone();
    const WHERE: [&str; 2] = ["SearchFolders", "ExcludedFolders"];
    two_columns(menu, ui, |menu, ui| {
    heading(ui, "Where to search", pal);
    ui.label(RichText::new("Tick what you want searched. Open a folder with its arrow to leave out what's inside.").size(12.5).color(pal.sub));
    ui.add_space(3.0);
    policy.lock(ui, &WHERE, |ui| card(ui, pal, |ui| {
        if let Some(home) = &home {
            tree_row(menu, ui, &view, home, "Your home folder", "", '\u{E80F}', 0, pal);
        }
        for drive in drives.iter().filter(|d| d.kind == DriveKind::Fixed) {
            tree_row(menu, ui, &view, &drive.root, &drive_title(drive), "", '\u{EDA2}', 0, pal);
        }
    }));

    // Anything added by hand that isn't inside the places above.
    let is_top = |root: &str| home.as_deref().is_some_and(|h| scope::same_path(h, root)) || drives.iter().any(|d| scope::same_path(&d.root, root));
    let inside_shown = |root: &str| home.as_deref().is_some_and(|h| scope::is_inside(root, h)) || drives.iter().any(|d| scope::is_inside(root, &d.root));
    let extras: Vec<String> = menu.cfg.roots.iter().filter(|r| !is_top(r) && !inside_shown(r)).cloned().collect();
    let mut added_place = None;
    policy.lock(ui, &WHERE, |ui| card(ui, pal, |ui| {
        for extra in &extras {
            tree_row(menu, ui, &view, extra, extra, "", FOLDER, 0, pal);
        }
        ui.horizontal(|ui| {
            if pill_button(ui, "Add a folder...", pal, false).clicked() {
                menu.ask_for_path(&ctx, true, Picked::SearchFolder);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let clicked = pill_button(ui, "Add", pal, false).clicked();
                let field = ui.add(TextEdit::singleline(&mut menu.scope_ui.new_place).desired_width(f32::INFINITY).hint_text("\\\\server\\share"));
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if (clicked || enter) && !menu.scope_ui.new_place.trim().is_empty() {
                    added_place = Some(menu.scope_ui.new_place.trim().to_string());
                }
            });
        });
    }));
    if let Some(place) = added_place {
        menu.add_search_place(place);
        menu.scope_ui.new_place.clear();
    }

    heading(ui, "Network and USB drives", pal);
    card(ui, pal, |ui| {
        policy.lock(ui, &["SearchNetworkDrives"], |ui| switch_line(ui, "Search network drives", "Includes any network drive as it gets attached. Big shares can take a while to read.", &mut menu.cfg.auto_network, pal));
        policy.lock(ui, &["SearchUsbDrives"], |ui| switch_line(ui, "Search USB drives", "Includes USB sticks and disks as they get plugged in.", &mut menu.cfg.auto_removable, pal));
        let attached: Vec<&Drive> = drives.iter().filter(|d| d.kind != DriveKind::Fixed).collect();
        if attached.is_empty() {
            ui.label(RichText::new("None attached right now.").size(12.5).color(pal.sub));
        } else {
            ui.add_space(2.0);
            ui.separator();
            for drive in attached {
                let glyph = if drive.kind == DriveKind::Network { '\u{E8CE}' } else { '\u{E88E}' };
                let where_to = if drive.kind == DriveKind::Network { drive.label.as_str() } else { "" };
                let title = if drive.kind == DriveKind::Network { format!("Network drive ({})", drive.root.trim_end_matches('\\')) } else { drive_title(drive) };
                tree_row(menu, ui, &view, &drive.root, &title, where_to, glyph, 0, pal);
            }
        }
    });
    }, |menu, ui| {
    heading(ui, "What to skip", pal);
    card(ui, pal, |ui| {
        for preset in SKIP_PRESETS {
            let mut on = preset.is_on(&menu.cfg);
            let before = on;
            policy.lock(ui, &[crate::policy::skip_key(preset.title).as_str(), "ExcludedFolderNames", "ExcludedFileTypes"], |ui| switch_line(ui, preset.title, preset.about, &mut on, pal));
            if on != before {
                preset.set(&mut menu.cfg, on);
            }
        }
        let mut skip_hidden = !menu.cfg.index_hidden;
        policy.lock(ui, &["IncludeHiddenFiles"], |ui| switch_line(ui, "Hidden files and folders", "Things Windows keeps out of sight.", &mut skip_hidden, pal));
        menu.cfg.index_hidden = !skip_hidden;
    });
    card(ui, pal, |ui| {
        policy.lock(ui, &["ExcludedFolderNames"], |ui| {
            ui.label(RichText::new("Also skip folders named").size(13.5).color(pal.text));
            tag_list(ui, &mut menu.cfg.exclude, pal);
            let mut text = std::mem::take(&mut menu.scope_ui.new_name);
            if add_rule_field(ui, &mut text, "A folder name, wherever it appears", pal) {
                menu.cfg.exclude.push(text.trim().to_string());
                text.clear();
            }
            menu.scope_ui.new_name = text;
        });
        ui.add_space(6.0);
        policy.lock(ui, &["ExcludedFileTypes"], |ui| {
            ui.label(RichText::new("Also skip file types").size(13.5).color(pal.text));
            tag_list(ui, &mut menu.cfg.excluded_exts, pal);
            let mut text = std::mem::take(&mut menu.scope_ui.new_ext);
            if add_rule_field(ui, &mut text, ".iso", pal) {
                let ext = format!(".{}", text.trim().trim_start_matches("*.").trim_start_matches('.').to_lowercase());
                if ext.len() > 1 && !menu.cfg.excluded_exts.contains(&ext) {
                    menu.cfg.excluded_exts.push(ext);
                }
                text.clear();
            }
            menu.scope_ui.new_ext = text;
        });
    });

    heading(ui, "Results", pal);
    card(ui, pal, |ui| {
        policy.lock(ui, &["ResultOrder"], |ui| {
            ui.label(RichText::new("Drag to choose what comes first").size(13.5).color(pal.text));
            drag_rows(ui, Id::unique("result-order"), &mut menu.cfg.category_order, 30.0, pal, |ui, category: &mut Category| {
                ui.label(RichText::new(category.label()).size(13.5).color(pal.text));
            });
        });
        ui.add_space(4.0);
        ui.separator();
        policy.lock(ui, &["ShowFolderUnderFile"], |ui| switch_line(ui, "Show the folder under each file", "", &mut menu.cfg.show_paths, pal));
        policy.lock(ui, &["OfferWebSearch"], |ui| switch_line(ui, "Offer a web search", "A last line under the results.", &mut menu.cfg.web_search, pal));
        policy.lock(ui, &["WebSearchAddress"], |ui| ui.horizontal(|ui| {
            ui.label(RichText::new("Web search with").size(13.5).color(pal.text));
            ui.add(TextEdit::singleline(&mut menu.cfg.web_url).desired_width(ui.available_width()));
        }));
    });
    });

    if let Some(path) = menu.scope_ui.toggle.take() {
        let hidden = view.hidden.clone();
        let is_hidden = move |p: &str| hidden.contains(&crate::matching::lower(p.trim_end_matches('\\')));
        scope::toggle(&mut menu.cfg, &autos, &path, &is_hidden);
    }
}
