//! The Quick actions and Bookmarks parts of the Search page of Settings: which quick actions search offers, which
//! browsers' bookmarks it reads, and the person's own bookmarks to add, change and remove.

use crate::bookmarks::{Bookmark, bookmarks_of, target_text};
use crate::config::{Config, Target};
use crate::customize::{card, heading};
use crate::i18n::tr;
use crate::search_page::switch_line;
use crate::settings::{QuickId, Value};
use crate::ui::{Menu, Picked};
use crate::widgets::*;
use egui::*;

/// A bookmark being written or changed.
#[derive(Default, Clone)]
pub struct Draft {
    /// Which of the person's bookmarks it changes; none for a new one.
    pub at: Option<usize>,
    pub name: String,
    pub target: String,
    pub args: String,
    pub folder: String,
}

#[derive(Default)]
pub struct BookmarkUi {
    pub draft: Option<Draft>,
}

impl BookmarkUi {
    /// Starts changing one of the person's bookmarks, or writing a new one.
    pub fn edit(&mut self, cfg: &Config, at: Option<usize>) {
        let mark = at.and_then(|at| cfg.bookmarks.get(at));
        let (target, args) = match mark.map(|m| &m.target) {
            Some(Target::Path { path, args }) => (path.clone(), args.clone()),
            Some(Target::App(id)) => (format!("app:{id}"), String::new()),
            None => (String::new(), String::new()),
        };
        self.draft = Some(Draft { at: mark.and(at), name: mark.map(|m| m.name.clone()).unwrap_or_default(), target, args, folder: mark.map(|m| m.folder.clone()).unwrap_or_default() });
    }
}

/// What a draft saves as, if it's complete.
pub fn bookmark_of(draft: &Draft) -> Option<Bookmark> {
    let target = draft.target.trim();
    if target.is_empty() {
        return None;
    }
    let target = match target.strip_prefix("app:") {
        Some(id) => Target::App(id.to_string()),
        None => Target::Path { path: target.trim_matches('"').to_string(), args: draft.args.trim().to_string() },
    };
    let name = if draft.name.trim().is_empty() { target_text(&target).rsplit(['\\', '/']).find(|p| !p.is_empty()).unwrap_or_default().to_string() } else { draft.name.trim().to_string() };
    Some(Bookmark { folder: draft.folder.trim().trim_matches('/').to_string(), name, target })
}

pub fn draw_quick_actions(menu: &mut Menu, ui: &mut Ui, pal: Pal) {
    let policy = menu.policy.clone();
    heading(ui, tr("Quick actions"), pal);
    card(ui, pal, |ui| {
        policy.lock(ui, &["SearchCalculator"], |ui| switch_line(ui, tr("Calculator"), tr("Sums, conversions of length, weight, temperature, volume, data sizes and time, and dates such as today + 30 days. Enter copies the result."), &mut menu.cfg.quick_calculator, pal));
        policy.lock(ui, &["SearchChangeWindowsSettings"], |ui| switch_line(ui, tr("Change Windows settings from search"), tr("Wi-Fi, Bluetooth, dark mode, do not disturb, mute, the sound output, volume and the other switches and sliders, with how they're set now."), &mut menu.cfg.search_windows_settings, pal));
        policy.lock(ui, &["SearchPowerActions"], |ui| switch_line(ui, tr("Power actions"), tr("Lock, sleep, sign out, restart and shut down. Restart and shut down ask you to press again."), &mut menu.cfg.search_power_actions, pal));
        policy.lock(ui, &["SearchWindowsSettingsPages"], |ui| switch_line(ui, tr("Shortcuts to Windows settings and tools"), tr("Pages of Windows Settings by their name, and tools such as Task Manager."), &mut menu.cfg.search_settings_pages, pal));
        policy.lock(ui, &["SearchClipboardHistory"], |ui| switch_line(ui, tr("Clipboard history"), tr("Find what you copied by its words, and paste it again with Enter."), &mut menu.cfg.quick_clipboard, pal));
        if menu.cfg.quick_clipboard && menu.settings.get(QuickId::ClipboardHistory) == Value::On(false) {
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Clipboard history is off in Windows.")).size(12.5).color(pal.sub));
                if ui.button(tr("Turn it on")).clicked() {
                    menu.settings.set(QuickId::ClipboardHistory, Value::On(true));
                }
            });
        }
    });
}

pub fn draw_bookmarks(menu: &mut Menu, ui: &mut Ui, pal: Pal) {
    let policy = menu.policy.clone();
    let ctx = ui.ctx().clone();
    heading(ui, tr("Bookmarks"), pal);
    card(ui, pal, |ui| {
        policy.lock(ui, &["SearchBookmarks"], |ui| switch_line(ui, tr("Bookmarks in search"), tr("Your own bookmarks, and those of your browsers. Found by name, address or folder, and taking part in the best match."), &mut menu.cfg.bookmarks_search, pal));
        if !menu.cfg.bookmarks_search {
            return;
        }
        ui.separator();
        policy.lock(ui, &["BookmarksFromBrowsers"], |ui| switch_line(ui, tr("From your browsers"), tr("Every profile of Chrome, Edge, Brave, Vivaldi, Opera, Helium and other Chromium browsers, and Firefox. Only read, never changed. A bookmark opens in the browser and profile it's from."), &mut menu.cfg.bookmarks_browsers, pal));
        if menu.cfg.bookmarks_browsers {
            let library = menu.providers.bookmarks.library.clone();
            let library = library.read().unwrap();
            policy.lock(ui, &["BookmarksLeftOut"], |ui| {
                ui.indent("bookmark-profiles", |ui| {
                    if library.profiles.is_empty() {
                        ui.label(RichText::new(tr("Looking for browsers with bookmarks...")).size(12.5).color(pal.sub));
                        ctx.request_repaint_after(std::time::Duration::from_secs(2));
                    }
                    for profile in &library.profiles {
                        let count = library.marks.iter().filter(|(at, _)| library.profiles.get(*at).is_some_and(|p| p.key == profile.key)).count();
                        let mut on = !menu.cfg.bookmarks_left_out.contains(&profile.key);
                        let before = on;
                        ui.horizontal(|ui| {
                            ui.set_min_height(28.0);
                            ui.label(RichText::new(profile.label()).size(13.5).color(pal.text));
                            ui.label(RichText::new(crate::trn!(count, "{n} bookmark", "{n} bookmarks")).size(12.0).color(pal.sub));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| switch(ui, &mut on, pal));
                        });
                        if on != before {
                            if on {
                                menu.cfg.bookmarks_left_out.retain(|k| k != &profile.key);
                            } else {
                                menu.cfg.bookmarks_left_out.push(profile.key.clone());
                            }
                        }
                    }
                });
            });
        }
        ui.separator();
        ui.label(RichText::new(tr("Your bookmarks")).size(13.5).color(pal.text));
        ui.label(RichText::new(tr("Web addresses, files, folders and commands. Right-click a result in search to add it here.")).size(12.0).color(pal.sub));
        // The organization's come first, and stay.
        for mark in bookmarks_of(policy.added("/bookmarks")) {
            bookmark_line(ui, &mark, true, pal);
        }
        let mut edit = None;
        let mut remove = None;
        let editing = menu.bookmark_ui.draft.as_ref().and_then(|d| d.at);
        let mut order: Vec<usize> = (0..menu.cfg.bookmarks.len()).collect();
        order.sort_by(|a, b| (&menu.cfg.bookmarks[*a].folder, &menu.cfg.bookmarks[*a].name).cmp(&(&menu.cfg.bookmarks[*b].folder, &menu.cfg.bookmarks[*b].name)));
        for at in order {
            if editing == Some(at) {
                continue;
            }
            match bookmark_line(ui, &menu.cfg.bookmarks[at], false, pal) {
                Some(true) => edit = Some(at),
                Some(false) => remove = Some(at),
                None => {}
            }
        }
        if let Some(at) = edit {
            menu.bookmark_ui.edit(&menu.cfg, Some(at));
        }
        if let Some(at) = remove {
            menu.cfg.bookmarks.remove(at);
            menu.bookmark_ui.draft = None;
        }
        ui.add_space(4.0);
        match menu.bookmark_ui.draft.clone() {
            Some(mut draft) => {
                let folders: Vec<String> = {
                    let mut all: Vec<String> = menu.cfg.bookmarks.iter().map(|b| b.folder.clone()).filter(|f| !f.is_empty()).collect();
                    all.sort();
                    all.dedup();
                    all
                };
                let (save, cancel, browse) = draft_form(ui, &mut draft, &folders, pal);
                if browse {
                    menu.ask_for_path(&ctx, false, Picked::Bookmark);
                }
                if cancel {
                    menu.bookmark_ui.draft = None;
                } else if save {
                    if let Some(mark) = bookmark_of(&draft) {
                        match draft.at.filter(|at| *at < menu.cfg.bookmarks.len()) {
                            Some(at) => menu.cfg.bookmarks[at] = mark,
                            None => menu.cfg.bookmarks.push(mark),
                        }
                        menu.bookmark_ui.draft = None;
                    }
                } else {
                    menu.bookmark_ui.draft = Some(draft);
                }
            }
            None => {
                if pill_button(ui, tr("Add a bookmark"), pal, false).clicked() {
                    menu.bookmark_ui.edit(&menu.cfg, None);
                }
            }
        }
    });
}

/// One bookmark: its name, folder and target. Some(true) to change it, Some(false) to remove it.
fn bookmark_line(ui: &mut Ui, mark: &Bookmark, managed: bool, pal: Pal) -> Option<bool> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.set_min_height(30.0);
        let glyph = if managed { '\u{E72E}' } else { '\u{E734}' };
        ui.label(RichText::new(glyph.to_string()).font(icon_font(12.0)).color(pal.sub));
        let folder = if mark.folder.is_empty() { String::new() } else { format!("{}/", mark.folder) };
        ui.label(RichText::new(format!("{folder}{}", mark.name)).size(13.5).color(pal.text)).on_hover_text(target_text(&mark.target));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if managed {
                ui.label(RichText::new(tr("Set by your organization")).size(12.0).color(pal.sub));
                return;
            }
            if ui.small_button(RichText::new('\u{E74D}'.to_string()).font(icon_font(12.0))).on_hover_text(tr("Remove")).clicked() {
                picked = Some(false);
            }
            if ui.small_button(RichText::new('\u{E70F}'.to_string()).font(icon_font(12.0))).on_hover_text(tr("Edit")).clicked() {
                picked = Some(true);
            }
            ui.add(Label::new(RichText::new(target_text(&mark.target)).size(12.0).color(pal.sub)).truncate()).on_hover_text(target_text(&mark.target));
        });
    });
    picked
}

/// The fields of a bookmark being written: (save, cancel, choose a file).
fn draft_form(ui: &mut Ui, draft: &mut Draft, folders: &[String], pal: Pal) -> (bool, bool, bool) {
    let (mut save, mut cancel, mut browse) = (false, false, false);
    let field = |ui: &mut Ui, label: &str, text: &mut String, hint: &str| {
        ui.horizontal(|ui| {
            ui.add_sized([110.0, 24.0], Label::new(RichText::new(label).size(13.0).color(pal.text)));
            text_box(ui, None, TextEdit::singleline(text).desired_width(ui.available_width()).hint_text(hint));
        });
    };
    Frame::NONE.fill(mix(pal.surface, pal.bg, 0.3)).corner_radius(pal.radius.min(6)).inner_margin(8.0).show(ui, |ui| {
        field(ui, tr("Name"), &mut draft.name, tr("Shown in search"));
        ui.horizontal(|ui| {
            ui.add_sized([110.0, 24.0], Label::new(RichText::new(tr("Opens")).size(13.0).color(pal.text)));
            if ui.button(tr("Choose a file...")).clicked() {
                browse = true;
            }
            text_box(ui, None, TextEdit::singleline(&mut draft.target).desired_width(ui.available_width()).hint_text(tr("A web address, file, folder or program")));
        });
        field(ui, tr("Arguments"), &mut draft.args, tr("For a program, if it needs any"));
        ui.horizontal(|ui| {
            ui.add_sized([110.0, 24.0], Label::new(RichText::new(tr("Folder")).size(13.0).color(pal.text)));
            text_box(ui, None, TextEdit::singleline(&mut draft.folder).desired_width((ui.available_width() - 130.0).max(80.0)).hint_text(tr("Work/Tools, or empty")));
            if !folders.is_empty() {
                ComboBox::from_id_salt("bookmark-folder").selected_text(tr("Folders")).width(120.0).show_ui(ui, |ui| {
                    for folder in folders {
                        if ui.selectable_label(draft.folder == *folder, folder).clicked() {
                            draft.folder = folder.clone();
                        }
                    }
                });
            }
        });
        ui.horizontal(|ui| {
            let ready = !draft.target.trim().is_empty();
            if ui.add_enabled(ready, Button::new(if draft.at.is_some() { tr("Save") } else { tr("Add") })).clicked() {
                save = true;
            }
            if ui.button(tr("Cancel")).clicked() {
                cancel = true;
            }
        });
    });
    (save, cancel, browse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bookmark_is_written_from_its_fields() {
        let draft = Draft { at: None, name: " ".into(), target: "C:\\Projects\\Plan.docx".into(), args: String::new(), folder: "/Work/".into() };
        let mark = bookmark_of(&draft).unwrap();
        assert_eq!(mark.name, "Plan.docx", "without a name it goes by the file's");
        assert_eq!(mark.folder, "Work");
        let command = bookmark_of(&Draft { name: "Ping".into(), target: "\"cmd.exe\"".into(), args: "/k ping localhost".into(), ..Default::default() }).unwrap();
        assert_eq!(command.target, Target::Path { path: "cmd.exe".into(), args: "/k ping localhost".into() });
        assert!(bookmark_of(&Draft::default()).is_none(), "nothing to open, nothing to save");
        let mut ui = BookmarkUi::default();
        let cfg = Config { roots: vec![], bookmarks: vec![command], ..Default::default() };
        ui.edit(&cfg, Some(0));
        let draft = ui.draft.unwrap();
        assert_eq!((draft.at, draft.name.as_str(), draft.args.as_str()), (Some(0), "Ping", "/k ping localhost"));
    }
}
