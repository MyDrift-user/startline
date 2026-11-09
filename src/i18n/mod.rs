//! The words Startline shows, in English, German or Italian.
//!
//! The English text in the code is the key: `tr("Settings")` gives "Einstellungen" in German. Text with values in it
//! names them, `trf!("Version {version}", version = v)`, and text that changes with a count has both forms,
//! `trn!(count, "{n} item", "{n} items")`. Text kept in tables is marked with `tx("…")` where it's written and looked
//! up with `tr` where it's shown. The catalogs are in `de/` and `it/`, one file per part of the app; the tests in
//! `tests.rs` make sure every marked text has a translation, and that no text the person sees is left unmarked.

// The formatting helpers are the ones every part of the app is meant to use, so some wait for their first caller.
#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

mod de;
mod it;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    De,
    It,
}

/// The choices for the `language` setting, as (settings file value, name in its own language). "system" follows the
/// language Windows is shown in.
pub const CHOICES: [(&str, &str); 4] = [("system", ""), ("en", "English"), ("de", "Deutsch"), ("it", "Italiano")];

static CURRENT: AtomicU8 = AtomicU8::new(0);

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::En, Lang::De, Lang::It];

    fn from_code(code: &str) -> Option<Lang> {
        match code.get(..2)?.to_ascii_lowercase().as_str() {
            "en" => Some(Lang::En),
            "de" => Some(Lang::De),
            "it" => Some(Lang::It),
            _ => None,
        }
    }

    /// "en", "de" or "it"
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::De => "de",
            Lang::It => "it",
        }
    }

    /// The culture Group Policy templates are filed under: "en-US", "de-DE", "it-IT".
    pub fn culture(self) -> &'static str {
        match self {
            Lang::En => "en-US",
            Lang::De => "de-DE",
            Lang::It => "it-IT",
        }
    }

    /// The language a `language` setting stands for: its own, or for "system" (or anything else) the system's.
    pub fn from_setting(value: &str) -> Lang {
        if value == "system" { system() } else { Lang::from_code(value).unwrap_or_else(system) }
    }
}

/// The language text is shown in now.
pub fn current() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::De,
        2 => Lang::It,
        _ => Lang::En,
    }
}

/// Show text in `lang` from now on. Cheap: the window can call it every frame.
pub fn set(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

/// The language Windows shows itself in, if Startline has it; English otherwise. Test hook: STARTLINE_TEST_LANG.
pub fn system() -> Lang {
    static SYSTEM: OnceLock<Lang> = OnceLock::new();
    if let Some(lang) = std::env::var("STARTLINE_TEST_LANG").ok().and_then(|v| Lang::from_code(&v)) {
        return lang;
    }
    *SYSTEM.get_or_init(|| system_code().and_then(|c| Lang::from_code(&c)).unwrap_or(Lang::En))
}

fn system_code() -> Option<String> {
    // The display language chosen in Windows's settings decides; the regional format only when it's one Startline has.
    let mut buf = [0u16; 85];
    let n = unsafe { windows::Win32::Globalization::GetUserDefaultLocaleName(&mut buf) };
    let ui = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
    let display = match ui & 0x3ff {
        0x07 => Some("de".to_string()),
        0x10 => Some("it".to_string()),
        0x09 => Some("en".to_string()),
        _ => None,
    };
    display.or_else(|| (n > 1).then(|| String::from_utf16_lossy(&buf[..n as usize - 1])))
}

fn table(lang: Lang) -> &'static HashMap<&'static str, &'static str> {
    static DE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    static IT: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    let (slot, parts) = match lang {
        Lang::It => (&IT, it::ALL),
        _ => (&DE, de::ALL),
    };
    slot.get_or_init(|| parts.iter().flat_map(|p| p.iter().copied()).collect())
}

/// `en` in the language text is shown in now. Text without a translation (which the tests don't let happen) stays
/// English.
pub fn tr(en: &'static str) -> &'static str {
    tr_in(current(), en)
}

/// `en` in `lang`, whatever is shown now.
pub fn tr_in(lang: Lang, en: &'static str) -> &'static str {
    match lang {
        Lang::En => en,
        lang => table(lang).get(en).copied().unwrap_or(en),
    }
}

/// Text kept in a table, marked for translation where it's written; `tr` gives it in the language shown.
pub const fn tx(en: &'static str) -> &'static str {
    en
}

/// Text that isn't known when the code is written, such as a table's entry passed along as a `String`: its
/// translation if it has one.
pub fn tr_dyn(en: &str) -> String {
    tr_dyn_in(current(), en)
}

/// The same in `lang`.
pub fn tr_dyn_in(lang: Lang, en: &str) -> String {
    match lang {
        Lang::En => en.to_string(),
        lang => table(lang).get(en).map_or_else(|| en.to_string(), |t| t.to_string()),
    }
}

/// `text` with each `{name}` replaced by the value given for it.
pub fn fill(text: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(at) = rest.find('{') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        match after.find('}').map(|end| (&after[..end], end)).filter(|(name, _)| args.iter().any(|(k, _)| k == name)) {
            Some((name, end)) => {
                let (_, v) = args.iter().find(|(k, _)| *k == name).expect("found above");
                out.push_str(&v.to_string());
                rest = &after[end + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Translated text with values in it: `trf!("Version {version}", version = v)`.
#[macro_export]
macro_rules! trf {
    ($en:literal $(, $k:ident = $v:expr)* $(,)?) => {
        $crate::i18n::fill($crate::i18n::tr($en), &[$((stringify!($k), &$v as &dyn ::std::fmt::Display)),*])
    };
}

/// Translated text for a count: `trn!(n, "{n} item", "{n} items")`, the first when there's one. German and Italian
/// have the same two forms. Further values as with `trf!`.
#[macro_export]
macro_rules! trn {
    ($n:expr, $one:literal, $other:literal $(, $k:ident = $v:expr)* $(,)?) => {{
        let n = $n;
        let text = if n == 1 { $crate::i18n::tr($one) } else { $crate::i18n::tr($other) };
        $crate::i18n::fill(text, &[("n", &$crate::i18n::fmt_count(n as usize) as &dyn ::std::fmt::Display) $(, (stringify!($k), &$v as &dyn ::std::fmt::Display))*])
    }};
}

/// The decimal separator: "2,5" in German and Italian.
pub fn decimal() -> char {
    if current() == Lang::En { '.' } else { ',' }
}

/// The thousands separator: "1.234" in German and Italian.
pub fn thousands() -> char {
    if current() == Lang::En { ',' } else { '.' }
}

/// 1,234,567 in English, 1.234.567 in German and Italian.
pub fn fmt_count(n: usize) -> String {
    fmt_count_in(current(), n)
}

pub fn fmt_count_in(lang: Lang, n: usize) -> String {
    let separator = if lang == Lang::En { ',' } else { '.' };
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(separator);
        }
        out.push(c);
    }
    out
}

/// A number with `places` decimals, as the language writes it: "3.4" or "3,4".
pub fn fmt_decimal(value: f64, places: usize) -> String {
    fmt_decimal_in(current(), value, places)
}

pub fn fmt_decimal_in(lang: Lang, value: f64, places: usize) -> String {
    let text = format!("{value:.places$}");
    if lang == Lang::En { text } else { text.replace('.', ",") }
}

/// A size in bytes: "12 KB", "2.5 GB", "2,5 GB" in German and Italian.
pub fn fmt_size(n: u64) -> String {
    fmt_size_in(current(), n)
}

pub fn fmt_size_in(lang: Lang, n: u64) -> String {
    const UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64 / 1024.0;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if v >= 100.0 { format!("{v:.0} {}", UNITS[unit]) } else { format!("{} {}", fmt_decimal_in(lang, v, 1), UNITS[unit]) }
}

/// A calendar date as the language writes it: "2026-11-02", "02.11.2026" in German, "02/11/2026" in Italian.
pub fn fmt_date(year: i32, month: u32, day: u32) -> String {
    fmt_date_in(current(), year, month, day)
}

pub fn fmt_date_in(lang: Lang, year: i32, month: u32, day: u32) -> String {
    match lang {
        Lang::En => format!("{year:04}-{month:02}-{day:02}"),
        Lang::De => format!("{day:02}.{month:02}.{year:04}"),
        Lang::It => format!("{day:02}/{month:02}/{year:04}"),
    }
}

/// A time of day, 24-hour in all three: "09:12".
pub fn fmt_time(hour: u32, minute: u32) -> String {
    format!("{hour:02}:{minute:02}")
}

/// Seconds since 1970 as a local date and time, as the language writes it.
pub fn fmt_unix(secs: u64) -> String {
    let local = crate::platform::local_time(secs);
    format!("{} {}", fmt_date(local.year, local.month, local.day), fmt_time(local.hour, local.minute))
}
