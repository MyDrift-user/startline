//! Mica, painted by the menu itself: the desktop picture, blurred far out of focus and tinted light or dark, lined up
//! with where the menu sits on the screen, as Windows 11's own Mica is. Windows can't be asked to put its Mica behind
//! this window: with some graphics drivers the menu's drawing then disappears behind a plain white or black sheet.
//! Painted here, it looks the same on every PC and follows the desktop picture when it changes.

use std::sync::{Arc, Mutex};

/// The material, small: it's drawn stretched over the whole screen, which only makes it smoother.
pub struct Picture {
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

/// The tint of Windows 11's Mica: its fallback colors, laid over the desktop picture.
const DARK_TINT: [f32; 3] = [32.0, 32.0, 32.0];
const LIGHT_TINT: [f32; 3] = [243.0, 243.0, 243.0];

/// The material made ready to draw, made again for light or dark mode and when the desktop picture changes.
#[derive(Default)]
pub struct Material {
    /// What was asked for last: dark or not, and which time the desktop picture was read.
    asked: Option<(bool, u64)>,
    /// Counts the changes of the desktop picture.
    changes: u64,
    made: Arc<Mutex<Option<Picture>>>,
    texture: Option<(egui::TextureHandle, [f32; 2])>,
}

impl Material {
    /// The desktop picture may have changed: it's read again the next time Mica is drawn.
    pub fn desktop_changed(&mut self) {
        self.changes += 1;
    }

    /// The material for light or dark mode with its size in pixels, or None while it's being made.
    pub fn texture(&mut self, ctx: &egui::Context, dark: bool) -> Option<(egui::TextureId, [f32; 2])> {
        let wanted = (dark, self.changes);
        if self.asked != Some(wanted) {
            self.asked = Some(wanted);
            let (slot, ctx) = (self.made.clone(), ctx.clone());
            std::thread::spawn(move || {
                crate::platform::com_init();
                let picture = desktop_picture().map_or_else(|| solid(desktop_color(), dark), |p| material(p, dark));
                *slot.lock().unwrap() = Some(picture);
                ctx.request_repaint();
            });
        }
        if let Some(picture) = self.made.lock().unwrap().take() {
            let image = egui::ColorImage::from_rgba_unmultiplied(picture.size, &picture.rgba);
            let size = [picture.size[0] as f32, picture.size[1] as f32];
            self.texture = Some((ctx.load_texture("mica", image, egui::TextureOptions::LINEAR), size));
        }
        self.texture.as_ref().map(|(t, size)| (t.id(), *size))
    }
}

/// The part of the material behind the window, as texture coordinates: the desktop picture fills the monitor, as
/// Windows shows it by default, and the window sees the piece of it under itself. `picture` is width and height.
pub fn uv_for(window: [f32; 4], monitor: [f32; 4], picture: [f32; 2]) -> egui::Rect {
    let [mx, my, mw, mh] = monitor;
    let [wx, wy, ww, wh] = window;
    let scale = (mw / picture[0].max(1.0)).max(mh / picture[1].max(1.0));
    let (shown_w, shown_h) = (picture[0] * scale, picture[1] * scale);
    let (left, top) = (mx + (mw - shown_w) / 2.0, my + (mh - shown_h) / 2.0);
    egui::Rect::from_min_max(egui::pos2((wx - left) / shown_w, (wy - top) / shown_h), egui::pos2((wx + ww - left) / shown_w, (wy + wh - top) / shown_h))
}

/// The desktop picture, read small. None for a plain color or a slideshow that can't be read.
fn desktop_picture() -> Option<Picture> {
    use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, BitmapTransform, ColorManagementMode, ExifOrientationMode};
    use windows::Storage::{FileAccessMode, StorageFile};
    use windows::Win32::UI::WindowsAndMessaging::{SPI_GETDESKWALLPAPER, SystemParametersInfoW};
    let mut path = [0u16; 520];
    unsafe { SystemParametersInfoW(SPI_GETDESKWALLPAPER, path.len() as u32, Some(path.as_mut_ptr() as *mut std::ffi::c_void), Default::default()) }.ok()?;
    let len = path.iter().position(|&c| c == 0).unwrap_or(0);
    if len == 0 {
        return None;
    }
    let file = StorageFile::GetFileFromPathAsync(&windows::core::HSTRING::from_wide(&path[..len])).ok()?.join().ok()?;
    let stream = file.OpenAsync(FileAccessMode::Read).ok()?.join().ok()?;
    let decoder = BitmapDecoder::CreateAsync(&stream).ok()?.join().ok()?;
    let (width, height) = (decoder.PixelWidth().ok()?.max(1), decoder.PixelHeight().ok()?.max(1));
    let scale = 96.0 / width.max(height) as f32;
    let (w, h) = (((width as f32 * scale).round() as u32).max(1), ((height as f32 * scale).round() as u32).max(1));
    let transform = BitmapTransform::new().ok()?;
    transform.SetScaledWidth(w).ok()?;
    transform.SetScaledHeight(h).ok()?;
    let pixels = decoder
        .GetPixelDataTransformedAsync(BitmapPixelFormat::Rgba8, BitmapAlphaMode::Ignore, &transform, ExifOrientationMode::RespectExifOrientation, ColorManagementMode::DoNotColorManage)
        .ok()?
        .join()
        .ok()?
        .DetachPixelData()
        .ok()?
        .to_vec();
    (pixels.len() == (w * h * 4) as usize).then_some(Picture { size: [w as usize, h as usize], rgba: pixels })
}

/// The desktop's plain color, when there's no picture.
fn desktop_color() -> [u8; 3] {
    use windows::Win32::Graphics::Gdi::{COLOR_DESKTOP, GetSysColor};
    let color = unsafe { GetSysColor(COLOR_DESKTOP) };
    [(color & 0xFF) as u8, ((color >> 8) & 0xFF) as u8, ((color >> 16) & 0xFF) as u8]
}

fn solid(color: [u8; 3], dark: bool) -> Picture {
    material(Picture { size: [1, 1], rgba: vec![color[0], color[1], color[2], 255] }, dark)
}

/// Blurs the picture far out of focus, takes out much of its color, and lays Mica's tint over it.
pub fn material(picture: Picture, dark: bool) -> Picture {
    let [w, h] = picture.size;
    let mut channels: Vec<[f32; 3]> = picture.rgba.chunks_exact(4).map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    // Three box blurs come close to a soft gaussian blur.
    let radius = (w.max(h) / 10).max(1);
    for _ in 0..3 {
        channels = box_blur(&channels, w, h, radius, true);
        channels = box_blur(&channels, w, h, radius, false);
    }
    let (tint, keep) = if dark { (DARK_TINT, 0.22) } else { (LIGHT_TINT, 0.32) };
    let rgba = channels
        .iter()
        .flat_map(|c| {
            let grey = 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2];
            let mut out = [0u8; 4];
            for (at, value) in c.iter().enumerate() {
                let muted = value * 0.6 + grey * 0.4;
                out[at] = (muted * keep + tint[at] * (1.0 - keep)).round().clamp(0.0, 255.0) as u8;
            }
            out[3] = 255;
            out
        })
        .collect();
    Picture { size: [w, h], rgba }
}

/// One pass of a box blur, across or down, with the edges held.
fn box_blur(source: &[[f32; 3]], w: usize, h: usize, radius: usize, across: bool) -> Vec<[f32; 3]> {
    let mut out = vec![[0.0; 3]; source.len()];
    let (lines, length) = if across { (h, w) } else { (w, h) };
    let at = |line: usize, i: usize| if across { line * w + i } else { i * w + line };
    for line in 0..lines {
        for i in 0..length {
            let (from, to) = (i.saturating_sub(radius), (i + radius).min(length - 1));
            let mut sum = [0.0f32; 3];
            for j in from..=to {
                let p = source[at(line, j)];
                sum = [sum[0] + p[0], sum[1] + p[1], sum[2] + p[2]];
            }
            let n = (to - from + 1) as f32;
            out[at(line, i)] = [sum[0] / n, sum[1] / n, sum[2] / n];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_material_is_the_desktop_picture_blurred_and_tinted() {
        // Half red, half blue: blurred, the two meet softly; tinted dark, nothing stays bright.
        let (w, h) = (20, 10);
        let rgba: Vec<u8> = (0..w * h).flat_map(|i| if i % w < w / 2 { [255, 0, 0, 255] } else { [0, 0, 255, 255] }).collect();
        let dark = material(Picture { size: [w, h], rgba: rgba.clone() }, true);
        assert!(dark.rgba.chunks_exact(4).all(|p| p[0] < 110 && p[2] < 110 && p[3] == 255), "dark Mica stays dark");
        let left = &dark.rgba[0..4];
        let middle = &dark.rgba[(w / 2) * 4..(w / 2) * 4 + 4];
        assert!(left[0] > left[2], "the red side keeps a little of its red");
        assert!((middle[0] as i32 - middle[2] as i32).abs() < (left[0] as i32 - left[2] as i32), "and the edge between is soft");
        let light = material(Picture { size: [w, h], rgba }, false);
        assert!(light.rgba.chunks_exact(4).all(|p| p[0] > 150 && p[1] > 150 && p[2] > 150), "light Mica stays light");
        assert_eq!(solid([0, 120, 215], true).size, [1, 1], "a plain desktop color works too");
    }

    #[test]
    fn the_window_sees_the_piece_of_the_picture_behind_it() {
        // A 16:9 picture on a 16:9 monitor: the window in the middle bottom sees that part.
        let uv = uv_for([960.0, 1040.0, 640.0, 400.0], [0.0, 0.0, 2560.0, 1440.0], [1600.0, 900.0]);
        assert!((uv.min.x - 0.375).abs() < 1e-4 && (uv.max.x - 0.625).abs() < 1e-4);
        assert!((uv.min.y - 1040.0 / 1440.0).abs() < 1e-4 && (uv.max.y - 1.0).abs() < 1e-3);
        // A square picture fills a wide monitor: it's cut at the top and bottom, as Windows' Fill does.
        let uv = uv_for([0.0, 0.0, 1920.0, 1080.0], [0.0, 0.0, 1920.0, 1080.0], [100.0, 100.0]);
        assert!(uv.min.y > 0.0 && uv.max.y < 1.0 && (uv.min.x).abs() < 1e-4);
        // A second monitor to the left.
        let uv = uv_for([-1920.0, 0.0, 1920.0, 1080.0], [-1920.0, 0.0, 1920.0, 1080.0], [1920.0, 1080.0]);
        assert!(uv.min.x.abs() < 1e-4 && (uv.max.x - 1.0).abs() < 1e-4);
    }
}
