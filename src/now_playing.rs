//! The "Now playing" card: what plays, with its cover and controls, above the footer of the Windows 11, Windows 10 and
//! Startline menus while the search box is empty.

use crate::media::Playing;
use crate::ui::Menu;
use crate::widgets::*;
use egui::*;

/// How tall the card is, with the gap above the footer.
pub const CARD_HEIGHT: f32 = 62.0;

impl Menu {
    /// What the card shows now; empty when it's switched off, something is being searched for, or nothing plays.
    pub(crate) fn now_playing(&self) -> Vec<Playing> {
        if !self.cfg.media_controls || !self.query.trim().is_empty() {
            return Vec::new();
        }
        self.media.playing()
    }

    pub(crate) fn draw_now_playing(&mut self, ui: &mut Ui, rect: Rect, playing: &[Playing], pal: Pal) {
        let Some(first) = playing.first() else { return };
        let at = self.media.chosen.as_ref().and_then(|id| playing.iter().position(|p| &p.app_id == id)).unwrap_or(0);
        let shown = playing.get(at).unwrap_or(first).clone();
        ui.painter().rect_filled(rect, pal.radius.min(8), mix(pal.surface, pal.bg, 0.2));
        ui.painter().rect_stroke(rect, pal.radius.min(8), Stroke::new(1.0, pal.line), StrokeKind::Inside);
        let inner = rect.shrink(8.0);
        // The cover, or a note where there's none or it's switched off.
        let cover_rect = Rect::from_min_size(inner.left_top(), Vec2::splat(inner.height()));
        let texture = shown.cover.as_ref().filter(|_| self.cfg.media_cover).map(|cover| {
            self.media
                .textures
                .entry(cover.key.clone())
                .or_insert_with(|| ui.ctx().load_texture(format!("cover:{}", cover.key), ColorImage::from_rgba_unmultiplied(cover.size, &cover.rgba), TextureOptions::LINEAR))
                .id()
        });
        // Pictures of what no longer plays are let go.
        let keep: Vec<String> = playing.iter().filter_map(|p| p.cover.as_ref().map(|c| c.key.clone())).collect();
        self.media.textures.retain(|key, _| keep.contains(key));
        match texture {
            Some(id) => {
                let size = shown.cover.as_ref().map_or([1, 1], |c| c.size);
                let fit = (cover_rect.width() / size[0] as f32).min(cover_rect.height() / size[1] as f32);
                let picture = Rect::from_center_size(cover_rect.center(), vec2(size[0] as f32 * fit, size[1] as f32 * fit));
                ui.painter().add(epaint::RectShape::filled(picture, pal.radius.min(4), Color32::WHITE).with_texture(id, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))));
            }
            None => {
                ui.painter().rect_filled(cover_rect, pal.radius.min(4), mix(pal.surface, pal.accent, 0.18));
                ui.painter().text(cover_rect.center(), Align2::CENTER_CENTER, '\u{E8D6}', icon_font(18.0), pal.text);
            }
        }
        // The buttons on the right: switch apps, previous, play or pause, next.
        let size = 30.0;
        let mut right = inner.right();
        let mut button = |ui: &mut Ui, glyph: char, tip: &str, enabled: bool| {
            right -= size;
            let area = Rect::from_min_size(pos2(right, inner.center().y - size / 2.0), Vec2::splat(size));
            let response = ui.interact(area, ui.scope_id().with(("media", glyph as u32)), if enabled { Sense::click() } else { Sense::hover() }).on_hover_text(tip);
            if enabled && response.hovered() {
                ui.painter().rect_filled(area, pal.radius.min(4), pal.hover);
            }
            ui.painter().text(area.center(), Align2::CENTER_CENTER, glyph, icon_font(14.0), if enabled { pal.text } else { pal.sub });
            enabled && response.clicked()
        };
        let next = button(ui, '\u{E893}', crate::i18n::tr("Next"), shown.can_next);
        let toggle = if shown.playing { button(ui, '\u{E769}', crate::i18n::tr("Pause"), shown.can_toggle) } else { button(ui, '\u{E768}', crate::i18n::tr("Play"), shown.can_toggle) };
        let previous = button(ui, '\u{E892}', crate::i18n::tr("Previous"), shown.can_previous);
        let switch = playing.len() > 1 && button(ui, '\u{E8AB}', &crate::trf!("Show the next app playing ({at} of {count})", at = at + 1, count = playing.len()), true);
        if next {
            self.media.next(&shown.app_id);
        }
        if toggle {
            self.media.toggle(&shown.app_id);
        }
        if previous {
            self.media.previous(&shown.app_id);
        }
        if switch {
            self.media.chosen = Some(playing[(at + 1) % playing.len()].app_id.clone());
        }
        // The title, then who and where.
        let text_left = cover_rect.right() + 10.0;
        let width = (right - 6.0 - text_left).max(40.0);
        let title = line_galley(ui, &shown.title, semibold(13.5), pal.text, width);
        let who = if shown.artist.is_empty() { shown.app.clone() } else { crate::trf!("{artist} in {app}", artist = shown.artist, app = shown.app) };
        let detail = line_galley(ui, &who, FontId::proportional(12.0), pal.sub, width);
        let total = title.size().y + detail.size().y + 2.0;
        let top = inner.center().y - total / 2.0;
        ui.painter().galley(pos2(text_left, top + title.size().y + 2.0), detail, pal.sub);
        ui.painter().galley(pos2(text_left, top), title, pal.text);
    }
}
