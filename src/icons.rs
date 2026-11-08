//! Real icons from Windows, fetched off-screen so drawing never waits on them.

use crate::platform;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::sync::{Arc, Condvar, Mutex};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Shell::*;
use windows::core::*;

/// Larger than most icons are drawn, so they stay crisp at bigger sizes and on high-DPI screens.
const ICON_PX: i32 = 96;

struct Loaded {
    key: String,
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

#[derive(Default)]
struct Queue {
    /// Newest first: what's on screen right now matters most.
    wanted: Vec<(String, String)>,
    done: Vec<Option<Loaded>>,
}

pub struct IconCache {
    textures: HashMap<String, egui::TextureHandle>,
    requested: HashSet<String>,
    shared: Arc<(Mutex<Queue>, Condvar)>,
}

impl IconCache {
    pub fn new(ctx: egui::Context) -> IconCache {
        let shared: Arc<(Mutex<Queue>, Condvar)> = Arc::default();
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("icons".into())
            .spawn(move || {
                platform::com_init();
                loop {
                    let (key, source) = {
                        let mut queue = worker.0.lock().unwrap();
                        while queue.wanted.is_empty() {
                            queue = worker.1.wait(queue).unwrap();
                        }
                        queue.wanted.pop().unwrap()
                    };
                    let loaded = load_icon(&source, key.starts_with("picture:")).map(|(width, height, pixels)| Loaded { key, width, height, pixels });
                    worker.0.lock().unwrap().done.push(loaded);
                    ctx.request_repaint();
                }
            })
            .expect("icon thread");
        IconCache { textures: HashMap::new(), requested: HashSet::new(), shared }
    }

    /// Asks for an icon: returns it if ready, otherwise fetches it for a later frame.
    /// `key` is what's shared between items (say, all PDFs); `source` is where to load it from.
    pub fn get(&mut self, key: &str, source: &str) -> Option<egui::TextureId> {
        if let Some(texture) = self.textures.get(key) {
            return Some(texture.id());
        }
        if self.requested.insert(key.to_string()) {
            let (lock, wake) = &*self.shared;
            lock.lock().unwrap().wanted.push((key.to_string(), source.to_string()));
            wake.notify_one();
        }
        None
    }

    /// Moves finished icons to the graphics card; call once per frame.
    pub fn upload(&mut self, ctx: &egui::Context) {
        let done = std::mem::take(&mut self.shared.0.lock().unwrap().done);
        for loaded in done.into_iter().flatten() {
            let size = [loaded.width, loaded.height];
            let image = if has_straight_alpha(&loaded.pixels) {
                egui::ColorImage::from_rgba_unmultiplied(size, &loaded.pixels)
            } else {
                egui::ColorImage::from_rgba_premultiplied(size, &loaded.pixels)
            };
            // Smaller copies of the picture keep edges smooth when it's drawn below its full size.
            let options = egui::TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..egui::TextureOptions::LINEAR };
            let texture = ctx.load_texture(&loaded.key, image, options);
            self.textures.insert(loaded.key, texture);
        }
    }

    pub fn is_idle(&self) -> bool {
        let queue = self.shared.0.lock().unwrap();
        queue.wanted.is_empty() && queue.done.is_empty()
    }
}

/// Windows hands over soft icon edges with the full color still in them, not yet faded by how
/// see-through each pixel is. Treated as already faded, those edge pixels come out far too
/// bright, which reads as a hard cut-out outline. Pictures that are already faded can never
/// have a color channel above their alpha, so one such pixel tells the two apart.
fn has_straight_alpha(rgba: &[u8]) -> bool {
    rgba.chunks_exact(4).any(|p| p[3] < 255 && p[0].max(p[1]).max(p[2]) > p[3])
}

/// The right shared key for a file: most types share one icon, programs and shortcuts have their own.
pub fn file_icon_key(path: &str, is_dir: bool) -> String {
    if is_dir {
        return "folder".into();
    }
    let name = path.rsplit('\\').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((_, ext)) => {
            let ext = ext.to_ascii_lowercase();
            if matches!(ext.as_str(), "exe" | "lnk" | "ico" | "url" | "msi" | "appref-ms") {
                format!("file:{path}")
            } else {
                format!("ext:{ext}")
            }
        }
        None => "ext:".into(),
    }
}

/// `picture` means the image itself, such as a profile picture, rather than the file's icon.
fn load_icon(source: &str, picture: bool) -> Option<(usize, usize, Vec<u8>)> {
    unsafe {
        let wide_source = platform::wide(source);
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(PCWSTR(wide_source.as_ptr()), None).ok()?;
        let flags = if picture { SIIGBF_RESIZETOFIT } else { SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK };
        let size = if picture { 128 } else { ICON_PX };
        let bitmap = factory.GetImage(SIZE { cx: size, cy: size }, flags).ok()?;
        let result = bitmap_pixels(bitmap);
        let _ = DeleteObject(bitmap.into());
        result
    }
}

unsafe fn bitmap_pixels(bitmap: HBITMAP) -> Option<(usize, usize, Vec<u8>)> {
    unsafe {
        let mut info = BITMAP::default();
        if GetObjectW(bitmap.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut info as *mut _ as *mut c_void)) == 0 {
            return None;
        }
        let (width, height) = (info.bmWidth as usize, info.bmHeight.unsigned_abs() as usize);
        if width == 0 || height == 0 {
            return None;
        }
        let mut header = BITMAPINFO::default();
        header.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        header.bmiHeader.biWidth = width as i32;
        header.bmiHeader.biHeight = -(height as i32); // top row first
        header.bmiHeader.biPlanes = 1;
        header.bmiHeader.biBitCount = 32;
        let mut pixels = vec![0u8; width * height * 4];
        let dc = CreateCompatibleDC(None);
        let lines = GetDIBits(dc, bitmap, 0, height as u32, Some(pixels.as_mut_ptr() as *mut c_void), &mut header, DIB_RGB_COLORS);
        let _ = DeleteDC(dc);
        if lines == 0 {
            return None;
        }
        // Windows hands out blue-green-red; old-style icons also leave alpha at zero everywhere.
        let opaque = pixels.chunks_exact(4).all(|p| p[3] == 0);
        for p in pixels.chunks_exact_mut(4) {
            p.swap(0, 2);
            if opaque {
                p[3] = 255;
            }
        }
        Some((width, height, pixels))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_edges_are_read_the_way_windows_sends_them() {
        // A white edge pixel, barely visible, with its full color still in it.
        assert!(has_straight_alpha(&[255, 255, 255, 30, 0, 0, 0, 255]));
        // The same pixel already faded.
        assert!(!has_straight_alpha(&[30, 30, 30, 30, 0, 0, 0, 255]));
        // Solid pictures can be read either way.
        assert!(!has_straight_alpha(&[200, 10, 90, 255]));
    }
}
