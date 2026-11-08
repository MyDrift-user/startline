//! Works out which setting a search is about and what the person wants done with it, so
//! "switch to light mode", "wlan aus", "volume 40" and "louder" each land on one live row.
//! It only reads the words: nothing changes until the person acts on the row.

use crate::matching;
use crate::settings::{Control, QuickId, Value};
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Want {
    Nothing,
    On,
    Off,
    Flip,
    Set(f32),
    Up,
    Down,
    Most,
    Least,
    Half,
    Pick(usize),
}

const TURN: &[&str] = &["turn", "set", "change", "make", "put", "schalte", "schalt", "schalten", "stell", "stelle", "stellen", "mach", "mache", "machen", "setze"];
const SWITCH: &[&str] = &["switch", "go", "wechsel", "wechsle", "wechseln"];
const TO: &[&str] = &["to", "zu", "auf", "in"];
const FLIP: &[&str] = &["toggle", "flip", "umschalten"];
const ENABLE: &[&str] = &["enable", "activate", "einschalten", "anschalten", "aktivieren"];
const DISABLE: &[&str] = &["disable", "deactivate", "ausschalten", "abschalten", "deaktivieren"];
const ON: &[&str] = &["on", "enabled", "an", "ein", "aktiviert"];
const OFF: &[&str] = &["off", "disabled", "aus", "no", "without", "kein", "keine", "deaktiviert"];
const UP: &[&str] = &["up", "higher", "more", "increase", "raise", "mehr", "höher", "erhöhen"];
const DOWN: &[&str] = &["down", "lower", "less", "decrease", "reduce", "weniger", "niedriger", "verringern"];
const MOST: &[&str] = &["max", "maximum", "full", "voll"];
const LEAST: &[&str] = &["min", "minimum"];
const HALF: &[&str] = &["half", "halb", "halbe"];
const UNITS: &[&str] = &["percent", "prozent", "minute", "minutes", "minuten", "mins", "min", "ms", "milliseconds"];
const SETTINGS_WORDS: &[&str] = &["setting", "settings", "einstellung", "einstellungen"];
const FILLER: &[&str] = &[
    "to", "the", "a", "my", "please", "i", "want", "can", "you", "how", "do", "does", "is", "it", "this", "of", "for", "in", "zu", "auf",
    "bitte", "der", "die", "das", "den", "dem", "mein", "meine", "ich", "will", "möchte", "von",
];

#[derive(Debug)]
pub struct Parsed {
    /// The words that carry meaning, spaced evenly, for finding whole phrases.
    raw: String,
    pub subject: Vec<String>,
    pub want: Want,
    number: Option<f32>,
    /// The words were clearly an order ("turn", "enable"), not just a name.
    command: bool,
    asks_for_settings: bool,
}

pub fn parse(tokens: &[String]) -> Parsed {
    let words: Vec<&str> = tokens
        .iter()
        .flat_map(|t| t.split(['-', ',', '!', '?', ';', ':', '"', '\'']))
        .map(|w| w.trim_matches('.'))
        .filter(|w| !w.is_empty())
        .collect();
    let meaningful: Vec<&str> = words.iter().copied().filter(|w| !FILLER.contains(w)).collect();
    let mut parsed = Parsed {
        raw: format!(" {} ", meaningful.join(" ")),
        subject: Vec::new(),
        want: Want::Nothing,
        number: None,
        command: false,
        asks_for_settings: false,
    };
    for (i, &word) in words.iter().enumerate() {
        let previous = i.checked_sub(1).map(|p| words[p]);
        let next = words.get(i + 1).copied();
        let after_number = previous.is_some_and(|p| number_of(p).is_some());
        if let Some(value) = number_of(word) {
            parsed.number = Some(value);
            parsed.want = Want::Set(value);
        } else if after_number && UNITS.contains(&word) {
            // "10 min" is a length of time, not "minimum".
        } else if SWITCH.contains(&word) {
            parsed.command = true;
            if parsed.want == Want::Nothing && next.is_some_and(|n| TO.contains(&n)) {
                parsed.want = Want::On;
            }
        } else if TURN.contains(&word) {
            parsed.command = true;
        } else if FLIP.contains(&word) {
            parsed.command = true;
            parsed.want = Want::Flip;
        } else if ENABLE.contains(&word) {
            parsed.command = true;
            parsed.want = Want::On;
        } else if DISABLE.contains(&word) {
            parsed.command = true;
            parsed.want = Want::Off;
        } else if word == "of" && previous.is_some_and(|p| TURN.contains(&p) || SWITCH.contains(&p)) {
            // A common slip: "turn of wifi".
            parsed.want = Want::Off;
        } else if keeps_number(parsed.want) && ON.contains(&word) {
            parsed.want = Want::On;
        } else if keeps_number(parsed.want) && OFF.contains(&word) {
            parsed.want = Want::Off;
        } else if keeps_number(parsed.want) && UP.contains(&word) {
            parsed.want = Want::Up;
        } else if keeps_number(parsed.want) && DOWN.contains(&word) {
            parsed.want = Want::Down;
        } else if keeps_number(parsed.want) && MOST.contains(&word) {
            parsed.want = Want::Most;
        } else if keeps_number(parsed.want) && LEAST.contains(&word) {
            parsed.want = Want::Least;
        } else if keeps_number(parsed.want) && HALF.contains(&word) {
            parsed.want = Want::Half;
        } else if SETTINGS_WORDS.contains(&word) {
            parsed.asks_for_settings = true;
        } else if UNITS.contains(&word) || FILLER.contains(&word) || (!keeps_number(parsed.want) && (ON.contains(&word) || OFF.contains(&word))) {
            // Says nothing about which setting is meant.
        } else {
            parsed.subject.push(word.to_string());
        }
    }
    parsed
}

/// A typed number is the most exact thing a person can say, so later words don't replace it.
fn keeps_number(want: Want) -> bool {
    !matches!(want, Want::Set(_))
}

fn number_of(word: &str) -> Option<f32> {
    let digits = word.strip_suffix('%').unwrap_or(word);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    digits.parse().ok()
}

struct Entry {
    id: QuickId,
    control: Control,
    label: String,
    /// The label without its hyphen, so "wifi" names "Wi-Fi" exactly.
    plain_label: String,
    names: Vec<String>,
    on_phrases: Vec<String>,
    off_phrases: Vec<String>,
    up_words: Vec<String>,
    down_words: Vec<String>,
    words: Vec<String>,
}

fn phrases(list: &str) -> Vec<String> {
    list.split(',').map(|p| matching::lower(p.trim())).filter(|p| !p.is_empty()).collect()
}

fn catalog() -> &'static [Entry] {
    static CATALOG: OnceLock<Vec<Entry>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        QuickId::ALL
            .iter()
            .map(|&id| {
                let info = id.info();
                let label = matching::lower(info.label);
                let up_words: Vec<String> = matching::tokens(info.up_words);
                let down_words: Vec<String> = matching::tokens(info.down_words);
                let names = phrases(info.names);
                let on_phrases = phrases(info.on_phrases);
                let off_phrases = phrases(info.off_phrases);
                let mut words = matching::tokens(info.keywords);
                for phrase in names.iter().chain(&on_phrases).chain(&off_phrases) {
                    words.extend(phrase.split_whitespace().filter(|w| !ON.contains(w) && !OFF.contains(w)).map(str::to_string));
                }
                words.extend(up_words.iter().chain(&down_words).cloned());
                words.sort_unstable();
                words.dedup();
                Entry {
                    id,
                    control: info.control,
                    plain_label: label.replace('-', ""),
                    label,
                    names,
                    on_phrases,
                    off_phrases,
                    up_words,
                    down_words,
                    words,
                }
            })
            .collect()
    })
}

fn word_score(word: &str, token: &str) -> Option<i32> {
    if word == token {
        Some(750)
    } else if word.starts_with(token) {
        Some(600)
    } else if token.len() >= 4 && word.contains(token) {
        Some(400)
    } else {
        None
    }
}

/// Edits between two words, counting a swap of neighbours as one. Stops caring above `limit`.
fn distance(a: &[char], b: &[char], limit: usize) -> usize {
    if a.len().abs_diff(b.len()) > limit {
        return limit + 1;
    }
    let mut rows = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in rows.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        rows[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = (a[i - 1] != b[j - 1]) as usize;
            let mut best = (rows[i - 1][j] + 1).min(rows[i][j - 1] + 1).min(rows[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(rows[i - 2][j - 2] + 1);
            }
            rows[i][j] = best;
        }
    }
    rows[a.len()][b.len()]
}

/// A slip of the fingers: "drak", "brightnes", "voulme". The first letter has to be right
/// (or swapped with the second), otherwise "house" would find "mouse".
fn is_typo_of(word: &str, token: &str) -> bool {
    let (w, t): (Vec<char>, Vec<char>) = (word.chars().collect(), token.chars().collect());
    if t.len() < 4 || w.len() < 4 {
        return false;
    }
    if w[0] != t[0] && !(w[0] == t[1] && w[1] == t[0]) {
        return false;
    }
    let limit = if t.len() >= 7 { 2 } else { 1 };
    distance(&w, &t, limit) <= limit
}

fn token_score(entry: &Entry, token: &str, typos: bool) -> Option<i32> {
    let in_label = matching::score_token(entry.label.as_bytes(), token.as_bytes()).filter(|s| *s > matching::LOOSE);
    let in_words = entry.words.iter().filter_map(|w| word_score(w, token)).max();
    if let Some(best) = in_label.into_iter().chain(in_words).max() {
        return Some(best);
    }
    if typos && entry.label.split([' ', '-']).chain(entry.words.iter().map(String::as_str)).any(|w| is_typo_of(w, token)) {
        return Some(320);
    }
    None
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Hit {
    pub id: QuickId,
    pub want: Want,
    pub score: i32,
    named: bool,
}

/// The longest phrase in the words that already says which way the setting should go, as
/// (turns it on, the phrase has its own on or off word).
fn phrase_side(entry: &Entry, raw: &str) -> Option<(bool, bool)> {
    let on = entry.on_phrases.iter().map(|p| (p, true));
    let off = entry.off_phrases.iter().map(|p| (p, false));
    on.chain(off)
        .filter(|(phrase, _)| raw.contains(&format!(" {phrase} ")))
        .max_by_key(|(phrase, _)| phrase.len())
        .map(|(phrase, side)| (side, phrase.split_whitespace().any(|w| ON.contains(&w) || OFF.contains(&w))))
}

fn match_entry(entry: &Entry, parsed: &Parsed, typos: bool) -> Option<Hit> {
    let is_slider = matches!(entry.control, Control::Slider { .. });
    if parsed.number.is_some() && !is_slider {
        return None;
    }
    let mut want = parsed.want;
    let mut tokens: Vec<&str> = parsed.subject.iter().map(String::as_str).collect();
    let before = tokens.len();
    if tokens.iter().any(|t| entry.up_words.iter().any(|w| w == t)) {
        want = Want::Up;
    }
    if tokens.iter().any(|t| entry.down_words.iter().any(|w| w == t)) {
        want = Want::Down;
    }
    tokens.retain(|t| !entry.up_words.iter().chain(&entry.down_words).any(|w| w == t));
    let mut needs_own_word = None;
    if let Control::Choice(options) = entry.control {
        for (index, option) in options.iter().enumerate() {
            let option = matching::lower(option);
            if tokens.contains(&option.as_str()) {
                want = Want::Pick(index);
                tokens.retain(|t| *t != option);
                needs_own_word = Some(option);
            }
        }
    }
    let score = if tokens.is_empty() {
        if before == 0 {
            return None;
        }
        // "left" alone is not about the taskbar, but "balanced" alone is about power.
        if needs_own_word.is_some_and(|option| !entry.words.contains(&option)) {
            return None;
        }
        700
    } else {
        let mut total = 0;
        for token in &tokens {
            total += token_score(entry, token, typos)?;
        }
        total / tokens.len() as i32
    };
    let joined = tokens.join(" ");
    let exact = !tokens.is_empty() && (joined == entry.label || joined == entry.plain_label || entry.names.contains(&joined));
    let consumed = tokens.len() != before;
    let phrase = if entry.control == Control::Toggle { phrase_side(entry, &parsed.raw) } else { None };
    if let Some((turns_on, own_word)) = phrase {
        want = match (want, turns_on) {
            _ if own_word => if turns_on { Want::On } else { Want::Off },
            (Want::Nothing | Want::On, true) => Want::On,
            (Want::Off, true) => Want::Off,
            (Want::Nothing | Want::On, false) => Want::Off,
            (Want::Off, false) => Want::On,
            (other, _) => other,
        };
    }
    let bonus = if exact { 400 } else { 0 } + if phrase.is_some() { 300 } else { 0 };
    Some(Hit { id: entry.id, want, score: score + bonus, named: exact || phrase.is_some() || consumed })
}

pub struct Found {
    pub hits: Vec<Hit>,
    /// The search is plainly about settings, so they should come before apps and files.
    pub lead: bool,
    pub subject: Vec<String>,
}

const MAX_LIVE: usize = 8;

pub fn find(tokens: &[String]) -> Found {
    let parsed = parse(tokens);
    let mut hits: Vec<Hit> = Vec::new();
    if parsed.subject.is_empty() {
        // "turn off" or "settings" on their own: offer what those words could apply to.
        if parsed.command || parsed.asks_for_settings {
            hits = catalog()
                .iter()
                .filter(|entry| match parsed.want {
                    Want::Nothing => true,
                    Want::On | Want::Off | Want::Flip => entry.control == Control::Toggle,
                    _ => matches!(entry.control, Control::Slider { .. }),
                })
                .map(|entry| Hit { id: entry.id, want: parsed.want, score: 900, named: true })
                .collect();
        }
    } else {
        hits = catalog().iter().filter_map(|e| match_entry(e, &parsed, false)).collect();
        if hits.is_empty() {
            hits = catalog().iter().filter_map(|e| match_entry(e, &parsed, true)).collect();
        }
    }
    hits.sort_by_key(|hit| std::cmp::Reverse(hit.score));
    hits.truncate(MAX_LIVE);
    let lead = !hits.is_empty() && (parsed.command || parsed.want != Want::Nothing || hits.iter().any(|h| h.named));
    Found { hits, lead, subject: parsed.subject }
}

/// What acting on the row would change the setting to. `None` means there is nothing to do.
pub fn resolve(control: Control, now: Value, want: Want) -> Option<Value> {
    let next = match (control, now) {
        (Control::Toggle, Value::On(on)) => Value::On(match want {
            Want::Nothing | Want::Flip => !on,
            Want::On | Want::Up | Want::Most => true,
            Want::Off | Want::Down | Want::Least => false,
            Want::Set(_) | Want::Half | Want::Pick(_) => return None,
        }),
        (Control::Slider { min, max, step, .. }, Value::Number(value)) => {
            let jump = ((max - min) / 10.0 / step).round().max(1.0) * step;
            let target = match want {
                Want::Set(number) => number,
                Want::Up => value + jump,
                Want::Down => value - jump,
                Want::Most => max,
                Want::Least | Want::Off => min,
                Want::Half => (min + max) / 2.0,
                Want::Nothing | Want::On | Want::Flip | Want::Pick(_) => return None,
            };
            Value::Number(((target.clamp(min, max) - min) / step).round() * step + min)
        }
        (Control::Choice(options), Value::Pick(current)) => {
            let last = options.len() - 1;
            Value::Pick(match want {
                Want::Pick(index) => index.min(last),
                Want::Nothing | Want::Flip => (current + 1) % options.len(),
                Want::Up => (current + 1).min(last),
                Want::Down => current.saturating_sub(1),
                Want::Most => last,
                Want::Least => 0,
                _ => return None,
            })
        }
        _ => return None,
    };
    (next != now).then_some(next)
}

pub fn format_number(value: f32, unit: &str) -> String {
    if value == 0.0 && unit == " min" {
        return "Never".into();
    }
    format!("{}{unit}", value.round() as i32)
}

/// The line under a setting's name: how it stands, or what Enter is about to do.
pub fn hint(control: Control, now: Value, want: Want) -> String {
    let asked = !matches!(want, Want::Nothing | Want::Flip);
    let next = resolve(control, now, want);
    match (control, now) {
        (_, Value::Loading) => "Checking...".into(),
        (_, Value::Unavailable) => "Not available on this PC. Enter opens its Windows page".into(),
        (Control::Toggle, Value::On(on)) => match (asked, next) {
            (true, Some(Value::On(true))) => "Enter turns it on".into(),
            (true, Some(Value::On(false))) => "Enter turns it off".into(),
            (true, _) => format!("Already {}", if on { "on" } else { "off" }),
            _ => (if on { "On" } else { "Off" }).into(),
        },
        (Control::Slider { unit, .. }, Value::Number(value)) => match (asked, next) {
            (true, Some(Value::Number(target))) => format!("Enter sets it to {}", format_number(target, unit)),
            (true, _) => format!("Already at {}", format_number(value, unit)),
            _ => "Drag to change".into(),
        },
        (Control::Choice(options), Value::Pick(current)) => match (asked, next) {
            (true, Some(Value::Pick(index))) => format!("Enter switches to {}", options[index]),
            _ => format!("Now: {}", options.get(current).copied().unwrap_or("")),
        },
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first(query: &str) -> (QuickId, Want) {
        let found = find(&matching::tokens(query));
        let hit = found.hits.first().unwrap_or_else(|| panic!("nothing found for {query}"));
        (hit.id, hit.want)
    }

    fn is_empty(query: &str) -> bool {
        find(&matching::tokens(query)).hits.is_empty()
    }

    #[test]
    fn plain_sentences_land_on_the_right_setting_and_direction() {
        assert_eq!(first("switch to dark mode"), (QuickId::DarkMode, Want::On));
        assert_eq!(first("switch to light mode"), (QuickId::DarkMode, Want::Off));
        assert_eq!(first("turn off light mode"), (QuickId::DarkMode, Want::On));
        assert_eq!(first("turn off dark mode"), (QuickId::DarkMode, Want::Off));
        assert_eq!(first("dark mode"), (QuickId::DarkMode, Want::Nothing));
        assert_eq!(first("turn off wifi"), (QuickId::Wifi, Want::Off));
        assert_eq!(first("turn of wifi"), (QuickId::Wifi, Want::Off));
        assert_eq!(first("wi-fi on"), (QuickId::Wifi, Want::On));
        assert_eq!(first("disable bluetooth"), (QuickId::Bluetooth, Want::Off));
        assert_eq!(first("how do i turn on bluetooth please"), (QuickId::Bluetooth, Want::On));
        assert_eq!(first("toggle sticky keys"), (QuickId::StickyKeys, Want::Flip));
        assert_eq!(first("\"switch to dark mode\""), (QuickId::DarkMode, Want::On));
    }

    #[test]
    fn german_works_too() {
        assert_eq!(first("wlan aus"), (QuickId::Wifi, Want::Off));
        assert_eq!(first("dunkelmodus an"), (QuickId::DarkMode, Want::On));
        assert_eq!(first("schalte helligkeit auf 30"), (QuickId::Brightness, Want::Set(30.0)));
        assert_eq!(first("lautstärke 20%"), (QuickId::Volume, Want::Set(20.0)));
        assert_eq!(first("lauter"), (QuickId::Volume, Want::Up));
    }

    #[test]
    fn phrases_that_mean_the_opposite_are_understood() {
        assert_eq!(first("mute"), (QuickId::Mute, Want::On));
        assert_eq!(first("unmute"), (QuickId::Mute, Want::Off));
        assert_eq!(first("turn off the sound"), (QuickId::Mute, Want::On));
        assert_eq!(first("turn off mic"), (QuickId::MicMute, Want::On));
        assert_eq!(first("hide taskbar"), (QuickId::AutoHideTaskbar, Want::On));
        assert_eq!(first("show taskbar"), (QuickId::AutoHideTaskbar, Want::Off));
        assert_eq!(first("show hidden files"), (QuickId::HiddenFiles, Want::On));
        assert_eq!(first("hide hidden files"), (QuickId::HiddenFiles, Want::Off));
        assert_eq!(first("show file extensions"), (QuickId::FileExtensions, Want::On));
    }

    #[test]
    fn numbers_and_directions_reach_sliders() {
        assert_eq!(first("set volume to 50"), (QuickId::Volume, Want::Set(50.0)));
        assert_eq!(first("volume 50%"), (QuickId::Volume, Want::Set(50.0)));
        assert_eq!(first("volume 50 percent"), (QuickId::Volume, Want::Set(50.0)));
        assert_eq!(first("volume up"), (QuickId::Volume, Want::Up));
        assert_eq!(first("max brightness"), (QuickId::Brightness, Want::Most));
        assert_eq!(first("dimmer"), (QuickId::Brightness, Want::Down));
        assert_eq!(first("screen off after 10 minutes"), (QuickId::ScreenTimeout, Want::Set(10.0)));
        assert_eq!(first("sleep after 30 min"), (QuickId::SleepTimeout, Want::Set(30.0)));
    }

    #[test]
    fn choices_can_be_named() {
        assert_eq!(first("taskbar left"), (QuickId::TaskbarAlign, Want::Pick(0)));
        assert_eq!(first("balanced"), (QuickId::PowerMode, Want::Pick(1)));
        assert_eq!(first("power mode best"), (QuickId::PowerMode, Want::Pick(2)));
        assert!(is_empty("left"));
    }

    #[test]
    fn small_slips_are_forgiven() {
        assert_eq!(first("drak mode").0, QuickId::DarkMode);
        assert_eq!(first("brightnes").0, QuickId::Brightness);
        assert_eq!(first("voulme").0, QuickId::Volume);
        assert_eq!(first("turn off bluetoth"), (QuickId::Bluetooth, Want::Off));
        assert!(is_empty("house"));
    }

    #[test]
    fn ordinary_searches_stay_out_of_it() {
        for query in ["note", "report 2025", "the report on taxes", "chrome", "holiday", "on", "aus", "50", "to the"] {
            assert!(is_empty(query), "{query} should not offer a setting");
        }
    }

    #[test]
    fn only_a_clear_ask_puts_settings_ahead_of_apps() {
        assert!(find(&matching::tokens("switch to dark mode")).lead);
        assert!(find(&matching::tokens("dark mode")).lead);
        assert!(find(&matching::tokens("wifi")).lead);
        assert!(!find(&matching::tokens("mo")).lead);
    }

    #[test]
    fn order_words_alone_list_what_they_could_apply_to() {
        let found = find(&matching::tokens("turn off"));
        assert_eq!(found.hits.len(), MAX_LIVE);
        assert!(found.hits.iter().all(|h| h.want == Want::Off && h.id.info().control == Control::Toggle));
        assert_eq!(find(&matching::tokens("settings")).hits.len(), MAX_LIVE);
    }

    #[test]
    fn every_setting_can_be_found_by_its_own_name() {
        for id in QuickId::ALL {
            let label = id.info().label;
            let found = find(&matching::tokens(label));
            assert!(found.hits.iter().any(|h| h.id == id), "{label} did not find itself");
        }
    }

    #[test]
    fn enter_does_what_was_asked_and_never_more() {
        assert_eq!(resolve(Control::Toggle, Value::On(false), Want::On), Some(Value::On(true)));
        assert_eq!(resolve(Control::Toggle, Value::On(true), Want::On), None);
        assert_eq!(resolve(Control::Toggle, Value::On(true), Want::Nothing), Some(Value::On(false)));
        assert_eq!(resolve(Control::Toggle, Value::Unavailable, Want::On), None);
        let percent = Control::Slider { min: 0.0, max: 100.0, step: 1.0, unit: "%" };
        assert_eq!(resolve(percent, Value::Number(70.0), Want::Set(50.0)), Some(Value::Number(50.0)));
        assert_eq!(resolve(percent, Value::Number(70.0), Want::Set(900.0)), Some(Value::Number(100.0)));
        assert_eq!(resolve(percent, Value::Number(70.0), Want::Up), Some(Value::Number(80.0)));
        assert_eq!(resolve(percent, Value::Number(70.0), Want::Set(70.0)), None);
        assert_eq!(resolve(percent, Value::Number(70.0), Want::Nothing), None);
        let double_click = Control::Slider { min: 200.0, max: 900.0, step: 50.0, unit: " ms" };
        assert_eq!(resolve(double_click, Value::Number(500.0), Want::Set(333.0)), Some(Value::Number(350.0)));
        let modes = Control::Choice(&["Saver", "Balanced", "Best"]);
        assert_eq!(resolve(modes, Value::Pick(1), Want::Pick(2)), Some(Value::Pick(2)));
        assert_eq!(resolve(modes, Value::Pick(2), Want::Nothing), Some(Value::Pick(0)));
    }

    #[test]
    fn the_line_under_the_name_says_what_enter_will_do() {
        assert_eq!(hint(Control::Toggle, Value::On(false), Want::On), "Enter turns it on");
        assert_eq!(hint(Control::Toggle, Value::On(true), Want::On), "Already on");
        assert_eq!(hint(Control::Toggle, Value::On(true), Want::Nothing), "On");
        let minutes = Control::Slider { min: 0.0, max: 120.0, step: 1.0, unit: " min" };
        assert_eq!(hint(minutes, Value::Number(10.0), Want::Set(0.0)), "Enter sets it to Never");
        assert_eq!(hint(minutes, Value::Number(10.0), Want::Set(10.0)), "Already at 10 min");
    }
}
