//! Right-click and account menus: icons beside the labels, and gone the moment you click
//! elsewhere, press Esc or the menu closes. Kept on the Menu rather than left to the UI
//! library, so reopening the Start menu can never bring an old one back.

use crate::actions::Do;
use crate::config::{Config, Target};
use crate::i18n::tr;
use crate::ui::{Intent, Menu, Page};
use crate::widgets::*;
use egui::*;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub enum Entry {
    Item { glyph: char, label: String, intent: Intent },
    Line,
}

pub struct Popup {
    at: Pos2,
    entries: Vec<Entry>,
    /// The frame it opened in: the click that opened it must not also close it.
    born: u64,
    hot: Option<usize>,
    in_settings: bool,
}

const WIDTH: f32 = 248.0;
const ROW: f32 = 32.0;
const LINE: f32 = 9.0;
const PADDING: f32 = 5.0;

pub fn height_of(entries: &[Entry]) -> f32 {
    entries.iter().map(|e| if matches!(e, Entry::Line) { LINE } else { ROW }).sum::<f32>() + PADDING * 2.0
}

fn item(glyph: char, label: &str, intent: Intent) -> Entry {
    Entry::Item { glyph, label: label.to_string(), intent }
}

/// What can be done with an app, file, folder or web address.
pub fn target_entries(cfg: &Config, name: &str, target: &Target, path: Option<&str>, on_start_menu: bool) -> Vec<Entry> {
    let open = |admin| Intent::Open { name: name.to_string(), target: target.clone(), admin };
    let is_web = matches!(target, Target::Path { path, .. } if path.contains("://"));
    let is_app = matches!(target, Target::App(_));
    let is_folder = matches!(target, Target::Path { path, .. } if !is_web && crate::platform::stat(path).is_some_and(|s| s.0));
    let pinned = cfg.is_pinned(target);
    let pin_label = if pinned { tr("Unpin from Start") } else { tr("Pin to Start") };
    let pin_glyph = if pinned { '\u{E77A}' } else { '\u{E718}' };
    let toggle = Intent::TogglePin { name: name.to_string(), target: target.clone() };

    let mut entries = vec![item('\u{E8E5}', tr("Open"), open(false))];
    if is_web {
        if let Target::Path { path, .. } = target {
            entries.push(Entry::Line);
            entries.push(item('\u{E8C8}', tr("Copy address"), Intent::Copy(path.clone())));
        }
        entries.push(item(pin_glyph, pin_label, toggle));
    } else {
        if !is_folder {
            entries.push(item('\u{E7EF}', tr("Run as administrator"), open(true)));
        }
        let file = path.map(str::to_string).filter(|_| !is_folder || !is_app);
        if let (Some(file), false) = (&file, is_app || is_folder) {
            entries.push(item('\u{E7AC}', tr("Open with..."), Intent::OpenWith(file.clone())));
        }
        entries.push(Entry::Line);
        if let Some(file) = &file {
            entries.push(item('\u{E838}', tr("Open file location"), Intent::Folder(file.clone())));
            entries.push(item('\u{E8C8}', tr("Copy path"), Intent::Copy(file.clone())));
        }
        entries.push(item('\u{E8AC}', tr("Copy name"), Intent::Copy(name.to_string())));
        entries.push(Entry::Line);
        entries.push(item(pin_glyph, pin_label, toggle));
        if is_app {
            entries.push(item('\u{E74D}', tr("Uninstall"), Intent::Run(Do::Open("ms-settings:appsfeatures"))));
        } else if let Some(file) = &file {
            entries.push(item('\u{E74D}', tr("Move to Recycle Bin"), Intent::Recycle(file.clone())));
        }
        if let Some(file) = &file {
            entries.push(item('\u{E946}', tr("Properties"), Intent::Properties(file.clone())));
        }
        let source = match target {
            Target::App(id) => format!("shell:AppsFolder\\{id}"),
            Target::Path { path, .. } => path.clone(),
        };
        entries.push(Entry::Line);
        entries.push(item('\u{E712}', tr("More options"), Intent::ShellMenu(source)));
    }
    if on_start_menu && pinned {
        entries.push(item('\u{E70F}', tr("Edit pinned apps..."), Intent::Show(Page::Customize(crate::customize::Tab::Pins))));
    }
    entries
}

/// Adding a result to the bookmarks, or changing or removing one of the person's own. The organization's can't be.
pub fn bookmark_entries(cfg: &Config, row: &crate::search::Row, target: &Target, managed: &[String]) -> Vec<Entry> {
    if !cfg.bookmarks_search {
        return Vec::new();
    }
    let own = cfg.bookmarks.iter().position(|b| &b.target == target && b.name == row.name);
    let theirs = crate::bookmarks::bookmarks_of(managed).iter().any(|b| &b.target == target && b.name == row.name);
    let mut entries = vec![Entry::Line];
    match own {
        Some(at) => {
            entries.push(item('\u{E70F}', tr("Edit bookmark..."), Intent::EditBookmark(at)));
            entries.push(item('\u{E74D}', tr("Remove from bookmarks"), Intent::RemoveBookmark(at)));
        }
        None if !theirs && !cfg.bookmarks.iter().any(|b| &b.target == target) => {
            entries.push(item('\u{E734}', tr("Add to bookmarks"), Intent::AddBookmark { name: row.name.clone(), target: target.clone() }));
        }
        None => return Vec::new(),
    }
    entries
}

/// The account menu behind your picture and name.
pub fn user_entries() -> Vec<Entry> {
    vec![
        item('\u{E77B}', tr("Account settings"), Intent::Run(Do::Open("ms-settings:yourinfo"))),
        item('\u{E8D7}', tr("Sign-in options"), Intent::Run(Do::Open("ms-settings:signinoptions"))),
        Entry::Line,
        item('\u{E748}', tr("Switch user"), Intent::Run(Do::SwitchUser)),
        item('\u{E72E}', tr("Lock"), Intent::Run(Do::Lock)),
        item('\u{F3B1}', tr("Sign out"), Intent::Run(Do::SignOut)),
    ]
}

pub fn setting_entries(page: &'static str) -> Vec<Entry> {
    vec![item('\u{E713}', tr("Open its Windows settings page"), Intent::Run(Do::Open(page)))]
}

/// Leaves out what a policy removed, along with any divider that no longer divides anything.
fn allowed_entries(policy: &crate::policy::Policies, entries: Vec<Entry>) -> Vec<Entry> {
    let mut kept: Vec<Entry> = Vec::with_capacity(entries.len());
    for entry in entries {
        match &entry {
            Entry::Line if kept.is_empty() || matches!(kept.last(), Some(Entry::Line)) => {}
            Entry::Item { intent, .. } if !policy.allows_intent(intent) => {}
            _ => kept.push(entry),
        }
    }
    if matches!(kept.last(), Some(Entry::Line)) {
        kept.pop();
    }
    kept
}

impl Menu {
    pub(crate) fn open_popup(&mut self, at: Pos2, entries: Vec<Entry>) {
        let entries = allowed_entries(&self.policy, entries);
        if entries.is_empty() {
            return;
        }
        self.popup = Some(Popup { at, entries, born: self.frames(), hot: None, in_settings: self.drawing_settings });
    }

    /// True when a menu is up and took the key, so nothing else should act on it.
    pub(crate) fn popup_keys(&mut self, ctx: &Context) -> bool {
        let Some(popup) = &mut self.popup else { return false };
        let (esc, up, down, enter) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Escape),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter),
            )
        });
        if esc {
            self.popup = None;
            return true;
        }
        let rows: Vec<usize> = popup.entries.iter().enumerate().filter(|(_, e)| matches!(e, Entry::Item { .. })).map(|(i, _)| i).collect();
        if up || down {
            let at = popup.hot.and_then(|h| rows.iter().position(|r| *r == h));
            let count = rows.len() as i32;
            let next = match (at, down) {
                (None, true) => 0,
                (None, false) => count - 1,
                (Some(i), true) => (i as i32 + 1).rem_euclid(count),
                (Some(i), false) => (i as i32 - 1).rem_euclid(count),
            };
            popup.hot = rows.get(next as usize).copied();
        }
        if enter {
            if let Some(Entry::Item { intent, .. }) = popup.hot.and_then(|h| popup.entries.get(h)).cloned() {
                self.choose(intent);
            }
        }
        true
    }

    /// Runs a choice. Anything that ends your session asks for a second click first.
    fn choose(&mut self, intent: Intent) {
        if let Intent::Run(what) = &intent {
            let armed = self.confirm.is_some_and(|(d, at)| d == *what && at.elapsed() < Duration::from_secs(5));
            if what.needs_confirm() && !armed {
                self.confirm = Some((*what, Instant::now()));
                return;
            }
        }
        self.popup = None;
        self.intents.push(intent);
    }

    pub(crate) fn draw_popup(&mut self, ctx: &Context, pal: Pal) {
        let Some(popup) = &self.popup else { return };
        if popup.in_settings != self.drawing_settings {
            return;
        }
        let entries = popup.entries.clone();
        let (born, hot) = (popup.born, popup.hot);
        let height = height_of(&entries);
        let bounds = ctx.content_rect();
        let x = popup.at.x.min(bounds.right() - WIDTH - 4.0).max(bounds.left() + 4.0);
        // Opens downward from the pointer, or upward if there isn't room below.
        let y = if popup.at.y + height > bounds.bottom() - 4.0 { (popup.at.y - height).max(bounds.top() + 4.0) } else { popup.at.y };
        let rect = Rect::from_min_size(pos2(x, y), vec2(WIDTH, height));

        let mut chosen = None;
        let mut hovered = None;
        Area::new(Id::unique("popup-menu")).order(Order::Foreground).fixed_pos(rect.min).show(ctx, |ui| {
            ui.set_min_size(rect.size());
            let round = pal.radius.saturating_add(2);
            ui.painter().add(ctx.global_style().visuals.popup_shadow.as_shape(rect, round));
            ui.painter().rect_filled(rect, round, pal.surface);
            ui.painter().rect_stroke(rect, round, Stroke::new(1.0, pal.line), StrokeKind::Inside);
            let mut top = rect.top() + PADDING;
            for (index, entry) in entries.iter().enumerate() {
                match entry {
                    Entry::Line => {
                        ui.painter().hline(rect.shrink2(vec2(10.0, 0.0)).x_range(), top + LINE / 2.0, Stroke::new(1.0, pal.line));
                        top += LINE;
                    }
                    Entry::Item { glyph, label, intent } => {
                        let row = Rect::from_min_size(pos2(rect.left() + PADDING, top), vec2(rect.width() - PADDING * 2.0, ROW));
                        let response = ui.interact(row, Id::unique(("popup-row", index)), Sense::click());
                        if response.hovered() {
                            hovered = Some(index);
                        }
                        let lit = response.hovered() || hot == Some(index);
                        if lit {
                            ui.painter().rect_filled(row, pal.radius, pal.hover);
                        }
                        let armed = matches!(intent, Intent::Run(what) if what.needs_confirm() && self.confirm.is_some_and(|(d, at)| d == *what && at.elapsed() < Duration::from_secs(5)));
                        let color = if armed { pal.accent } else { pal.text };
                        ui.painter().text(pos2(row.left() + 16.0, row.center().y), Align2::CENTER_CENTER, *glyph, icon_font(14.0), color);
                        let text = if armed { crate::trf!("Click again to confirm: {action}", action = label) } else { label.clone() };
                        ui.painter().text(pos2(row.left() + 36.0, row.center().y - ink_nudge(13.0)), Align2::LEFT_CENTER, text, FontId::proportional(13.0), color);
                        if response.clicked() {
                            chosen = Some(intent.clone());
                        }
                        top += ROW;
                    }
                }
            }
        });
        if let (Some(index), Some(popup)) = (hovered, self.popup.as_mut()) {
            popup.hot = Some(index);
        }
        if let Some(intent) = chosen {
            self.choose(intent);
            return;
        }
        // A press anywhere else closes it. If it was a right-click, a new menu opens by itself.
        let pressed_outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p)));
        if pressed_outside && born != self.frames() {
            self.popup = None;
        }
    }
}
