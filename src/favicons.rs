//! Sites' own little pictures for bookmarks, read from what the browsers already keep: Chromium's Favicons database
//! in each profile and Firefox's favicons.sqlite, both from copies in the temp folder. Nothing is fetched from the
//! internet.

use crate::sqlite::Db;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// A site's picture, as straight RGBA.
pub struct Favicon {
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

/// The pictures found, by page address and by "host:" and the site's name.
pub type Favicons = HashMap<String, Arc<Favicon>>;

/// The site's name in an address: "www.rust-lang.org" in "https://www.rust-lang.org/learn".
pub fn host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit('@').next().unwrap_or(host);
    host.split(':').next().unwrap_or(host).to_ascii_lowercase()
}

/// The picture for an address: its own, or else any of the same site's.
pub fn lookup<'a>(favicons: &'a Favicons, url: &str) -> Option<&'a Arc<Favicon>> {
    favicons.get(url).or_else(|| favicons.get(&format!("host:{}", host(url))))
}

/// A PNG, or an ICO holding a PNG or a 32-bit picture, as RGBA. Anything else (SVG, older bitmaps) gives none.
pub fn decode(data: &[u8]) -> Option<Favicon> {
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if data.starts_with(PNG) {
        return png(data);
    }
    // An ICO: its pictures listed up front, the one nearest 32 pixels wins.
    if data.len() < 6 || data[0..4] != [0, 0, 1, 0] {
        return None;
    }
    let count = u16::from_le_bytes([data[4], data[5]]) as usize;
    let entries = (0..count).filter_map(|at| {
        let entry = data.get(6 + at * 16..6 + at * 16 + 16)?;
        let width = if entry[0] == 0 { 256 } else { entry[0] as usize };
        let size = u32::from_le_bytes(entry[8..12].try_into().ok()?) as usize;
        let offset = u32::from_le_bytes(entry[12..16].try_into().ok()?) as usize;
        Some((width, data.get(offset..offset.checked_add(size)?)?))
    });
    let (_, picture) = entries.min_by_key(|(width, _)| (*width as i64 - 32).abs())?;
    if picture.starts_with(PNG) { png(picture) } else { bitmap(picture) }
}

fn png(data: &[u8]) -> Option<Favicon> {
    let mut decoder = ::png::Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(::png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    let (width, height) = (info.width as usize, info.height as usize);
    if width == 0 || height == 0 || width > 512 || height > 512 {
        return None;
    }
    let pixels = &buffer[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        ::png::ColorType::Rgba => pixels.to_vec(),
        ::png::ColorType::Rgb => pixels.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        ::png::ColorType::GrayscaleAlpha => pixels.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        ::png::ColorType::Grayscale => pixels.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        ::png::ColorType::Indexed => return None,
    };
    (rgba.len() == width * height * 4).then_some(Favicon { size: [width, height], rgba })
}

/// A 32-bit picture inside an ICO: a bitmap header, then the rows bottom up in blue, green, red and alpha.
fn bitmap(data: &[u8]) -> Option<Favicon> {
    let read = |at: usize| data.get(at..at + 4).map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let header = read(0)? as usize;
    let width = read(4)?;
    let height = read(8)? / 2;
    let bits = u16::from_le_bytes([*data.get(14)?, *data.get(15)?]);
    if bits != 32 || !(1..=256).contains(&width) || !(1..=256).contains(&height) {
        return None;
    }
    let (width, height) = (width as usize, height as usize);
    let pixels = data.get(header..header + width * height * 4)?;
    let mut rgba = vec![0u8; width * height * 4];
    for y in 0..height {
        let from = &pixels[(height - 1 - y) * width * 4..(height - y) * width * 4];
        for (x, p) in from.chunks_exact(4).enumerate() {
            rgba[(y * width + x) * 4..(y * width + x) * 4 + 4].copy_from_slice(&[p[2], p[1], p[0], p[3]]);
        }
    }
    Some(Favicon { size: [width, height], rgba })
}

/// Which of each page's and each site's pictures to read: the one nearest 32 pixels wide. `rows` are page address,
/// picture id and width; only pages of the bookmarked sites count.
fn choose(rows: Vec<Vec<Option<String>>>, urls: &HashSet<String>) -> (HashMap<String, i64>, HashSet<i64>) {
    let hosts: HashSet<String> = urls.iter().map(|u| host(u)).collect();
    let mut best: HashMap<String, (i64, i64)> = HashMap::new();
    for row in rows.into_iter().filter(|r| r.len() == 3) {
        let (Some(page), Some(id)) = (row[0].clone(), row[1].as_deref().and_then(|i| i.parse::<i64>().ok())) else { continue };
        let width: i64 = row[2].as_deref().and_then(|w| w.parse().ok()).unwrap_or(16);
        let site = host(&page);
        if !hosts.contains(&site) {
            continue;
        }
        let distance = (width - 32).abs();
        let mut keep = |key: String| {
            if best.get(&key).is_none_or(|(_, d)| distance < *d) {
                best.insert(key, (id, distance));
            }
        };
        if urls.contains(&page) {
            keep(page);
        }
        keep(format!("host:{site}"));
    }
    let ids = best.values().map(|(id, _)| *id).collect();
    (best.into_iter().map(|(key, (id, _))| (key, id)).collect(), ids)
}

/// Reads the chosen pictures and puts each where it belongs.
fn gather(db: &Db, chosen: HashMap<String, i64>, ids: HashSet<i64>, sql: &str) -> Favicons {
    if ids.is_empty() {
        return Favicons::new();
    }
    let list = ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
    let Some(rows) = db.cells(&sql.replace("{ids}", &list)) else { return Favicons::new() };
    let pictures: HashMap<i64, Arc<Favicon>> = rows
        .into_iter()
        .filter_map(|row| {
            let id = std::str::from_utf8(row.first()?.as_deref()?).ok()?.parse().ok()?;
            Some((id, Arc::new(decode(row.get(1)?.as_deref()?)?)))
        })
        .collect();
    chosen.into_iter().filter_map(|(key, id)| Some((key, pictures.get(&id)?.clone()))).collect()
}

/// The pictures in a copy of a Chromium profile's Favicons database, for these bookmarked addresses.
pub fn read_chromium(db: &Db, urls: &HashSet<String>) -> Favicons {
    let Some(rows) = db.rows("SELECT m.page_url, b.id, b.width FROM icon_mapping m JOIN favicon_bitmaps b ON b.icon_id = m.icon_id") else { return Favicons::new() }; // i18n-ignore: SQL
    let (chosen, ids) = choose(rows, urls);
    gather(db, chosen, ids, "SELECT id, image_data FROM favicon_bitmaps WHERE id IN ({ids})") // i18n-ignore: SQL
}

/// The pictures in a copy of a Firefox profile's favicons.sqlite, for these bookmarked addresses.
pub fn read_firefox(db: &Db, urls: &HashSet<String>) -> Favicons {
    let sql = "SELECT p.page_url, i.id, i.width FROM moz_pages_w_icons p JOIN moz_icons_to_pages t ON t.page_id = p.id JOIN moz_icons i ON i.id = t.icon_id"; // i18n-ignore: SQL
    let Some(rows) = db.rows(sql) else { return Favicons::new() };
    let (chosen, ids) = choose(rows, urls);
    gather(db, chosen, ids, "SELECT id, data FROM moz_icons WHERE id IN ({ids})") // i18n-ignore: SQL
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2 by 1 PNG: one red pixel, one see-through blue one.
    fn tiny_png() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = ::png::Encoder::new(&mut out, 2, 1);
            encoder.set_color(::png::ColorType::Rgba);
            encoder.set_depth(::png::BitDepth::Eight);
            encoder.write_header().unwrap().write_image_data(&[255, 0, 0, 255, 0, 0, 255, 128]).unwrap();
        }
        out
    }

    #[test]
    fn pictures_are_read_from_png_and_ico() {
        let png = tiny_png();
        let picture = decode(&png).unwrap();
        assert_eq!((picture.size, picture.rgba.as_slice()), ([2, 1], &[255, 0, 0, 255, 0, 0, 255, 128][..]));
        // The same PNG inside an ICO.
        let mut ico = vec![0, 0, 1, 0, 1, 0, 2, 1, 0, 0, 1, 0, 32, 0];
        ico.extend((png.len() as u32).to_le_bytes());
        ico.extend(22u32.to_le_bytes());
        ico.extend(&png);
        assert_eq!(decode(&ico).unwrap().size, [2, 1]);
        assert!(decode(b"<svg></svg>").is_none(), "a drawing in words isn't read");
        assert!(decode(&[0, 0, 1, 0, 5, 0]).is_none(), "nor a broken ICO");
    }

    #[test]
    fn sites_are_named_from_their_address() {
        assert_eq!(host("https://www.Rust-Lang.org/learn?x=1"), "www.rust-lang.org");
        assert_eq!(host("http://user@intranet:8080/a"), "intranet");
        assert_eq!(host("example.com"), "example.com");
    }

    #[test]
    fn chromium_and_firefox_pictures_are_found_for_bookmarked_sites() {
        if !crate::sqlite::available() {
            return;
        }
        let png = tiny_png();
        let hex: String = png.iter().map(|b| format!("{b:02X}")).collect();
        let urls: HashSet<String> = ["https://www.rust-lang.org/".to_string(), "https://docs.example.com/guide".to_string()].into();
        let path = |name: &str| std::env::temp_dir().join(format!("startline-test-{name}-{}.sqlite", std::process::id()));

        let chromium = path("favicons");
        let _ = std::fs::remove_file(&chromium);
        {
            let db = Db::open(&chromium, true).unwrap();
            assert!(db.exec(&format!(
                "CREATE TABLE favicons (id INTEGER PRIMARY KEY, url LONGVARCHAR NOT NULL, icon_type INTEGER DEFAULT 1);
                 CREATE TABLE icon_mapping (id INTEGER PRIMARY KEY, page_url LONGVARCHAR NOT NULL, icon_id INTEGER);
                 CREATE TABLE favicon_bitmaps (id INTEGER PRIMARY KEY, icon_id INTEGER NOT NULL, last_updated INTEGER DEFAULT 0, image_data BLOB, width INTEGER DEFAULT 0, height INTEGER DEFAULT 0, last_requested INTEGER DEFAULT 0);
                 INSERT INTO favicons VALUES (1, 'https://www.rust-lang.org/favicon.ico', 1), (2, 'https://docs.example.com/icon.png', 1), (3, 'https://other.example/x.ico', 1);
                 INSERT INTO icon_mapping VALUES (1, 'https://www.rust-lang.org/', 1), (2, 'https://docs.example.com/other-page', 2), (3, 'https://other.example/', 3);
                 INSERT INTO favicon_bitmaps (id, icon_id, image_data, width, height) VALUES (10, 1, X'{hex}', 16, 16), (11, 1, X'{hex}', 32, 32), (12, 2, X'{hex}', 16, 16), (13, 3, X'{hex}', 16, 16);"
            )));
            let found = read_chromium(&db, &urls);
            assert!(found.contains_key("https://www.rust-lang.org/"));
            assert!(lookup(&found, "https://docs.example.com/guide").is_some(), "another page of the site lends its picture");
            assert!(!found.contains_key("host:other.example"), "only bookmarked sites' pictures are kept");
        }
        let _ = std::fs::remove_file(&chromium);

        let firefox = path("ffavicons");
        let _ = std::fs::remove_file(&firefox);
        {
            let db = Db::open(&firefox, true).unwrap();
            assert!(db.exec(&format!(
                "CREATE TABLE moz_icons (id INTEGER PRIMARY KEY, icon_url TEXT NOT NULL, fixed_icon_url_hash INTEGER NOT NULL DEFAULT 0, width INTEGER NOT NULL DEFAULT 0, root INTEGER NOT NULL DEFAULT 0, color INTEGER, expire_ms INTEGER NOT NULL DEFAULT 0, data BLOB);
                 CREATE TABLE moz_pages_w_icons (id INTEGER PRIMARY KEY, page_url TEXT NOT NULL, page_url_hash INTEGER NOT NULL DEFAULT 0);
                 CREATE TABLE moz_icons_to_pages (page_id INTEGER NOT NULL, icon_id INTEGER NOT NULL, expire_ms INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (page_id, icon_id));
                 INSERT INTO moz_icons (id, icon_url, width, data) VALUES (1, 'https://www.rust-lang.org/favicon.ico', 32, X'{hex}');
                 INSERT INTO moz_pages_w_icons (id, page_url) VALUES (1, 'https://www.rust-lang.org/');
                 INSERT INTO moz_icons_to_pages (page_id, icon_id) VALUES (1, 1);"
            )));
            let found = read_firefox(&db, &urls);
            assert_eq!(lookup(&found, "https://www.rust-lang.org/").map(|f| f.size), Some([2, 1]));
            assert!(lookup(&found, "https://docs.example.com/guide").is_none(), "no picture, so the star stays");
        }
        let _ = std::fs::remove_file(&firefox);
    }
}
