//! The Search settings page: tick the places to search in a folder tree, flip simple skip
//! rules, and see the result at once. Changes re-read the folders by themselves.

use crate::config::{Category, ResultKind};
use crate::customize::{card, heading};
use crate::i18n::tr;
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
    children: Arc<Mutex<HashMap<String, Vec<(String, u32)>>>>,
    /// Folders seen with the hidden or system attribute, by lower-case path.
    hidden: Arc<Mutex<HashMap<String, u32>>>,
    drives: Arc<Mutex<Vec<Drive>>>,
    drives_read: Option<Instant>,
    counts: Vec<(String, usize)>,
    counts_read: Option<Instant>,
    toggle: Option<String>,
    new_name: String,
    new_ext: String,
    new_pattern: String,
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
            for (name, attrs) in &found {
                if *attrs != 0 {
                    known.insert(crate::matching::lower(&format!("{}\\{name}", path.trim_end_matches('\\'))), *attrs);
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
    skip_attrs: u32,
    hidden: HashMap<String, u32>,
    children: HashMap<String, Vec<(String, u32)>>,
    counts: Vec<(String, usize)>,
}

impl View {
    fn with_scope<R>(&self, read: impl FnOnce(&Scope) -> R) -> R {
        let is_hidden = |path: &str| self.hidden.get(&crate::matching::lower(path.trim_end_matches('\\'))).is_some_and(|a| a & self.skip_attrs != 0);
        read(&Scope {
            roots: &self.all_roots,
            excluded_paths: &self.excluded_paths,
            excluded_names: &self.excluded_names,
            include_hidden: self.skip_attrs == 0,
            is_hidden: &is_hidden,
        })
    }
}

/// A number of results, with a slider.
fn count_line(ui: &mut Ui, title: &str, value: &mut usize, range: std::ops::RangeInclusive<usize>, pal: Pal) {
    ui.horizontal(|ui| {
        ui.set_min_height(30.0);
        let room = ui.available_width() * 0.45;
        if ui.painter().layout_no_wrap(title.to_string(), FontId::proportional(13.5), pal.text).size().x <= room {
            ui.label(RichText::new(title).size(13.5).color(pal.text));
        } else {
            ui.vertical(|ui| {
                ui.set_max_width(room);
                ui.add(Label::new(RichText::new(title).size(13.5).color(pal.text)).wrap());
            });
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().slider_width = 130.0;
            ui.add(Slider::new(value, range).text_color(pal.text));
        });
    });
}

pub(crate) fn switch_line(ui: &mut Ui, title: &str, about: &str, value: &mut bool, pal: Pal) {
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
        Tick::On => tr("Searched. Click to leave it out."),
        Tick::Off => tr("Not searched. Click to include it."),
        Tick::Some => tr("Partly searched. Click to change the whole folder."),
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
        tr("Skipped by a rule").to_string()
    } else if let Some((_, count)) = view.counts.iter().find(|(root, _)| scope::same_path(root, path)) {
        crate::trn!(*count, "{n} item", "{n} items")
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
                ui.label(RichText::new(format!("    {}", tr("Reading…"))).size(12.0).color(pal.sub));
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
        DriveKind::Fixed => tr("Local Disk"),
        DriveKind::Network => tr("Network Drive"),
        DriveKind::Removable => tr("USB Drive"),
    };
    if drive.label.is_empty() { format!("{kind} ({letter})") } else { format!("{} ({letter})", drive.label) }
}

/// The rules in a list as chips: the person's own can be clicked away, the organization's can't. Items a switch
/// above stands for are left to that switch. `problem` says what's wrong with an item, if anything.
fn tag_list(ui: &mut Ui, list: &mut Vec<String>, locked: &[String], problem: impl Fn(&str) -> Option<String>, pal: Pal) {
    let shown = |item: &str| !scope::in_a_preset(item) || locked.iter().any(|l| l.eq_ignore_ascii_case(item));
    // An empty row would leave a gap between the label and its box.
    if !list.iter().any(|item| shown(item)) {
        return;
    }
    let mut remove = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for (index, item) in list.iter().enumerate() {
            if !shown(item) {
                continue;
            }
            let theirs = locked.iter().any(|l| l.eq_ignore_ascii_case(item));
            let trouble = problem(item);
            // Room for a lock or a warning, drawn below in the icon font.
            let text = match (theirs, &trouble) {
                (true, _) => format!("      {item}"),
                (false, Some(_)) => format!("      {item}   ×"),
                (false, None) => format!("{item}   ×"),
            };
            let response = chip(ui, &text, None, false, pal);
            let mark = if theirs { Some(('\u{E72E}', pal.sub)) } else { trouble.as_ref().map(|_| ('\u{E7BA}', Color32::from_rgb(222, 120, 96))) };
            if let Some((glyph, color)) = mark {
                ui.painter().text(pos2(response.rect.left() + 16.0, response.rect.center().y), Align2::CENTER_CENTER, glyph, icon_font(11.0), color);
            }
            let response = match (theirs, trouble) {
                (true, _) => response.on_hover_text(tr("Set by your organization")),
                (false, Some(why)) => response.on_hover_text(crate::trf!("{why}\nIgnored until it's fixed. Click to remove it.", why = why)),
                (false, None) => response.on_hover_text(tr("Click to remove this rule")),
            };
            if !theirs && response.clicked() {
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
            add = pill_button(ui, tr("Add"), pal, false).clicked();
            let field = text_box(ui, None, TextEdit::singleline(text).desired_width(f32::INFINITY).hint_text(hint));
            add |= field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        });
    });
    add && !text.trim().is_empty()
}

pub fn draw(menu: &mut Menu, ui: &mut Ui, pal: Pal) {
    menu.scope_ui.refresh(&menu.index, ui.ctx());
    let ctx = ui.ctx().clone();

    crate::customize::page_header(ui, tr("Search"), tr("Where search looks, what it leaves out and how results are shown."), pal);

    // How things stand right now.
    let status = menu.index.status.clone();
    let line = if status.walking.load(Ordering::Relaxed) {
        ctx.request_repaint_after(Duration::from_millis(300));
        crate::trf!("Reading your folders… {count} so far", count = crate::i18n::fmt_count(status.walked.load(Ordering::Relaxed)))
    } else {
        crate::trn!(menu.file_count(), "{n} file or folder ready to search", "{n} files and folders ready to search")
    };
    ui.label(RichText::new(line).size(13.0).color(pal.text));
    ui.add_space(8.0);

    let drives = menu.scope_ui.drives.lock().unwrap().clone();
    let autos = auto_roots(menu.cfg.auto_network, menu.cfg.auto_removable);
    let view = View {
        all_roots: scope::all_roots(&menu.cfg, &autos),
        excluded_paths: menu.cfg.excluded_paths.clone(),
        excluded_names: menu.cfg.exclude.clone(),
        skip_attrs: menu.cfg.skip_attrs(),
        hidden: menu.scope_ui.hidden.lock().unwrap().clone(),
        children: menu.scope_ui.children.lock().unwrap().clone(),
        counts: menu.scope_ui.counts.clone(),
    };

    let home = home_folder();
    let policy = menu.policy.clone();
    // The administrator's folders never found only add to people's own: they can still untick folders.
    const WHERE: [&str; 1] = ["SearchFolders"];
    two_columns(menu, ui, |menu, ui| {
    heading(ui, tr("Where to search"), pal);
    ui.label(RichText::new(tr("Tick what you want searched. Open a folder with its arrow to leave out what's inside.")).size(12.5).color(pal.sub));
    ui.add_space(3.0);
    policy.lock(ui, &WHERE, |ui| card(ui, pal, |ui| {
        if let Some(home) = &home {
            tree_row(menu, ui, &view, home, tr("Your home folder"), "", '\u{E80F}', 0, pal);
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
            if pill_button(ui, tr("Add a folder…"), pal, false).clicked() {
                menu.ask_for_path(&ctx, true, Picked::SearchFolder);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let clicked = pill_button(ui, tr("Add"), pal, false).clicked();
                let field = text_box(ui, None, TextEdit::singleline(&mut menu.scope_ui.new_place).desired_width(f32::INFINITY).hint_text("\\\\server\\share"));
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

    heading(ui, tr("What's indexed"), pal);
    card(ui, pal, |ui| {
        policy.lock(ui, &["SearchLocalDrives"], |ui| switch_line(ui, tr("Index local drives"), tr("Names and dates of files and folders on this PC's own drives, read gently and kept up to date as they change."), &mut menu.cfg.index_local, pal));
        policy.lock(ui, &["SearchNetworkDrives"], |ui| switch_line(ui, tr("Index network drives"), tr("Network drives and shares, including \\\\server\\share places added by hand. Read at most once a day while the PC is idle, one folder at a time, with no watch on the server."), &mut menu.cfg.auto_network, pal));
        policy.lock(ui, &["SearchUsbDrives"], |ui| switch_line(ui, tr("Index USB drives"), tr("USB sticks, disks and memory cards, as they get plugged in."), &mut menu.cfg.auto_removable, pal));
        ui.label(RichText::new(tr("Startline reads only the names and dates of files and folders, never what's in them. Apps and settings are found whatever is switched off here.")).size(12.0).color(pal.sub));
        let attached: Vec<&Drive> = drives.iter().filter(|d| d.kind != DriveKind::Fixed).collect();
        if attached.is_empty() {
            ui.label(RichText::new(tr("None attached right now.")).size(12.5).color(pal.sub));
        } else {
            ui.add_space(2.0);
            ui.separator();
            for drive in attached {
                let glyph = if drive.kind == DriveKind::Network { '\u{E8CE}' } else { '\u{E88E}' };
                let where_to = if drive.kind == DriveKind::Network { drive.label.as_str() } else { "" };
                let title = if drive.kind == DriveKind::Network { crate::trf!("Network Drive ({letter})", letter = drive.root.trim_end_matches('\\')) } else { drive_title(drive) };
                tree_row(menu, ui, &view, &drive.root, &title, where_to, glyph, 0, pal);
            }
        }
    });
    }, |menu, ui| {
    heading(ui, tr("Never found"), pal);
    card(ui, pal, |ui| {
        for preset in SKIP_PRESETS {
            let mut on = preset.is_on(&menu.cfg);
            let before = on;
            policy.lock(ui, &[preset.key], |ui| switch_line(ui, tr(preset.title), tr(preset.about), &mut on, pal));
            if on != before {
                preset.set(&mut menu.cfg, on);
            }
        }
        policy.lock(ui, &["SearchSkipHidden"], |ui| switch_line(ui, tr("Hidden files and folders"), tr("Things Windows keeps out of sight."), &mut menu.cfg.skip_hidden, pal));
        policy.lock(ui, &["SearchSkipSystem"], |ui| switch_line(ui, tr("System files"), tr("What Windows marks as part of the system, such as desktop.ini."), &mut menu.cfg.skip_system, pal));
    });
    let no_problem = |_: &str| None;
    card(ui, pal, |ui| {
        if menu.cfg.excluded_paths.iter().any(|p| !p.is_empty()) {
            ui.label(RichText::new(tr("Folders")).size(13.5).color(pal.text));
            tag_list(ui, &mut menu.cfg.excluded_paths, policy.added("/excluded_paths"), no_problem, pal);
            ui.add_space(6.0);
        }
        ui.label(RichText::new(tr("Folders named")).size(13.5).color(pal.text));
        tag_list(ui, &mut menu.cfg.exclude, policy.added("/exclude"), no_problem, pal);
        let mut text = std::mem::take(&mut menu.scope_ui.new_name);
        if add_rule_field(ui, &mut text, tr("A folder name, wherever it appears"), pal) {
            menu.cfg.exclude.push(text.trim().to_string());
            text.clear();
        }
        menu.scope_ui.new_name = text;
        ui.add_space(6.0);
        ui.label(RichText::new(tr("File types")).size(13.5).color(pal.text));
        tag_list(ui, &mut menu.cfg.excluded_exts, policy.added("/excluded_exts"), no_problem, pal);
        let mut text = std::mem::take(&mut menu.scope_ui.new_ext);
        if add_rule_field(ui, &mut text, ".iso", pal) {
            let ext = crate::policy::file_type(&text);
            if ext.len() > 1 && !menu.cfg.excluded_exts.contains(&ext) {
                menu.cfg.excluded_exts.push(ext);
            }
            text.clear();
        }
        menu.scope_ui.new_ext = text;
        ui.add_space(6.0);
        ui.label(RichText::new(tr("Paths matching")).size(13.5).color(pal.text));
        ui.label(RichText::new(tr("Regular expressions, checked against the whole path in any case.")).size(12.0).color(pal.sub));
        let problem = |p: &str| crate::index::pattern(p).err().map(|e| e.to_string().lines().last().unwrap_or(tr("Not a valid pattern")).trim().to_string());
        tag_list(ui, &mut menu.cfg.skip_patterns, policy.added("/skip_patterns"), problem, pal);
        let mut text = std::mem::take(&mut menu.scope_ui.new_pattern);
        if add_rule_field(ui, &mut text, r"\\Payroll(\\|$)", pal) {
            if !menu.cfg.skip_patterns.contains(&text) {
                menu.cfg.skip_patterns.push(text.trim().to_string());
            }
            text.clear();
        }
        menu.scope_ui.new_pattern = text;
    });

    heading(ui, tr("Results"), pal);
    card(ui, pal, |ui| {
        policy.lock(ui, &["SearchResults"], |ui| {
            ui.label(RichText::new(tr("Drag to choose what comes first")).size(13.5).color(pal.text));
            // People and mail are in the list only while their Microsoft 365 feature is on; the rest keep their place after.
            let shown = |kind: Category| match kind {
                Category::People => crate::m365::feature_on(&menu.cfg, crate::m365::Feature::People),
                Category::Mail => crate::m365::feature_on(&menu.cfg, crate::m365::Feature::Mail),
                _ => true,
            };
            let (mut listed, hidden): (Vec<ResultKind>, Vec<ResultKind>) = menu.cfg.results.iter().partition(|r| shown(r.kind));
            drag_rows(ui, Id::unique("result-order"), &mut listed, 34.0, pal, |ui, result: &mut ResultKind| {
                ui.label(RichText::new(result.kind.label()).size(13.5).color(if result.on { pal.text } else { pal.sub }));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| switch(ui, &mut result.on, pal));
            });
            listed.extend(hidden);
            if listed != menu.cfg.results {
                menu.cfg.results = listed;
            }
        });
        ui.add_space(4.0);
        ui.separator();
        policy.lock(ui, &["SearchMaxApps"], |ui| count_line(ui, tr("Most apps"), &mut menu.cfg.max_apps, 1..=30, pal));
        policy.lock(ui, &["SearchMaxSettings"], |ui| count_line(ui, tr("Most settings"), &mut menu.cfg.max_settings, 1..=30, pal));
        policy.lock(ui, &["SearchMaxFiles"], |ui| count_line(ui, tr("Most files and folders"), &mut menu.cfg.max_files, 5..=200, pal));
        ui.separator();
        policy.lock(ui, &["SearchBestMatch"], |ui| switch_line(ui, tr("Best match at the top"), tr("The one result whose name fits what was typed best, whatever its kind, comes first."), &mut menu.cfg.best_match, pal));
        policy.lock(ui, &["SearchShowFolders"], |ui| switch_line(ui, tr("Show the folder under each file"), "", &mut menu.cfg.show_paths, pal));
        policy.lock(ui, &["SearchChangeWindowsSettings"], |ui| switch_line(ui, tr("Change Windows settings from search"), tr("Dark mode, volume, Wi-Fi and the other switches and sliders."), &mut menu.cfg.search_windows_settings, pal));
        policy.lock(ui, &["SearchWindowsSettingsPages"], |ui| switch_line(ui, tr("Shortcuts to Windows settings and tools"), tr("Pages of Windows Settings, and tools such as Task Manager."), &mut menu.cfg.search_settings_pages, pal));
        ui.separator();
        policy.lock(ui, &["SearchWebUseAddress"], |ui| switch_line(ui, tr("Search the web with a fixed address"), tr("Off, web searches open your default browser with the search engine you set up there. The address below then only serves when Startline doesn't know your browser."), &mut menu.cfg.web_fixed, pal));
        let fixed = menu.cfg.web_fixed;
        ui.indent("web-address", |ui| {
            policy.lock(ui, &["SearchWebName"], |ui| ui.add_enabled_ui(fixed, |ui| ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Name of the web search")).size(13.5).color(pal.text));
                text_box(ui, None, TextEdit::singleline(&mut menu.cfg.web_name).desired_width(ui.available_width()));
            })));
            policy.lock(ui, &["SearchWebAddress"], |ui| ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Web search address")).size(13.5).color(pal.text));
                text_box(ui, None, TextEdit::singleline(&mut menu.cfg.web_url).desired_width(ui.available_width()));
            }));
        });
    });

    heading(ui, tr("Search box"), pal);
    card(ui, pal, |ui| {
        policy.lock(ui, &["SearchRegex"], |ui| switch_line(ui, tr("Treat searches as regular expressions"), tr("The whole search is one pattern, matched against names whatever the case."), &mut menu.cfg.regex_search, pal));
        policy.lock(ui, &["SearchShowRegexSwitch"], |ui| {
            ui.indent("regex-switch", |ui| switch_line(ui, tr("Its switch in the search box"), tr("Hidden, searches are never regular expressions."), &mut menu.cfg.show_regex_switch, pal))
        });
        policy.lock(ui, &["SearchShowHelp"], |ui| switch_line(ui, tr("Help button (?) in the search box"), tr("Shows what you can type: \"exact text\", wildcards, -without, ext:, kind:, size:, modified: and regular expressions."), &mut menu.cfg.show_search_help, pal));
        policy.lock(ui, &["SearchFileTypeButtons"], |ui| switch_line(ui, tr("File type buttons"), tr("Narrow the results to one kind of file."), &mut menu.cfg.show_file_buttons, pal));
        policy.lock(ui, &["SearchShowTime"], |ui| switch_line(ui, tr("How long a search took"), "", &mut menu.cfg.show_timing, pal));
        policy.lock(ui, &["SearchShowHints"], |ui| switch_line(ui, tr("Keyboard hints under the results"), "", &mut menu.cfg.show_search_hints, pal));
    });
    });

    if let Some(path) = menu.scope_ui.toggle.take() {
        let (hidden, mask) = (view.hidden.clone(), view.skip_attrs);
        let is_hidden = move |p: &str| hidden.get(&crate::matching::lower(p.trim_end_matches('\\'))).is_some_and(|a| a & mask != 0);
        scope::toggle(&mut menu.cfg, &autos, &path, &is_hidden);
    }
}
