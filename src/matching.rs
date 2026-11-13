//! How well a name matches what was typed. Shared by apps, settings and files so
//! ranking feels the same everywhere.

const EXACT: i32 = 1000;
const STEM_EXACT: i32 = 950;
const PREFIX: i32 = 800;
const WORD_START: i32 = 650;
const CONTAINS: i32 = 450;
/// Below anything found as one piece of text: "vsc" for Visual Studio Code.
const INITIALS: i32 = 350;
/// Nothing scores at or below this: letters scattered through a name never match.
pub const LOOSE: i32 = 200;

fn is_boundary(prev: u8, cur: u8) -> bool {
    matches!(prev, b' ' | b'-' | b'_' | b'.' | b'(' | b'[' | b'+' | b'&' | b',') && cur != b' '
}

/// Scores one lowercase query word against a lowercase name. `None` means no match.
pub fn score_token(name: &[u8], token: &[u8]) -> Option<i32> {
    if token.is_empty() {
        return Some(0);
    }
    if name == token {
        return Some(EXACT);
    }
    if let Some(dot) = name.iter().rposition(|&b| b == b'.') {
        if dot > 0 && &name[..dot] == token {
            return Some(STEM_EXACT);
        }
    }
    if name.starts_with(token) {
        return Some(PREFIX);
    }
    let mut first_contains = None;
    let finder = memchr::memmem::Finder::new(token);
    for pos in finder.find_iter(name) {
        if pos > 0 && is_boundary(name[pos - 1], name[pos]) {
            return Some(WORD_START);
        }
        if first_contains.is_none() {
            first_contains = Some(pos);
        }
    }
    if let Some(pos) = first_contains {
        return Some(CONTAINS - (pos.min(40) as i32));
    }
    // Only the starts of words, in order. Letters from the middle of words, as "wifi" in "Windows-Fotoanzeige", aren't a match.
    initials_match(name, token).then_some(INITIALS)
}

/// "vsc" finds "Visual Studio Code".
fn initials_match(name: &[u8], token: &[u8]) -> bool {
    if token.len() < 2 {
        return false;
    }
    let mut want = token.iter();
    let mut next = want.next();
    let mut prev = b' ';
    for &b in name {
        if let Some(&t) = next {
            if b != b' ' && is_boundary(prev, b) && b == t {
                next = want.next();
            }
        }
        prev = b;
    }
    next.is_none()
}

/// Lowercases without allocating anything extra for plain ASCII names.
pub fn lower(text: &str) -> String {
    if text.is_ascii() {
        text.to_ascii_lowercase()
    } else {
        text.to_lowercase()
    }
}

pub fn tokens(query: &str) -> Vec<String> {
    lower(query).split_whitespace().map(str::to_string).collect()
}

/// Scores all words against `name`, with `extra` searched too (for aliases, file names of apps).
pub fn score_all(name: &str, extra: &str, tokens: &[String]) -> Option<i32> {
    if tokens.is_empty() {
        return None;
    }
    let mut total = 0;
    for token in tokens {
        let in_name = score_token(name.as_bytes(), token.as_bytes());
        let in_extra = if extra.is_empty() {
            None
        } else {
            score_token(extra.as_bytes(), token.as_bytes()).map(|s| (s - 150).max(1))
        };
        total += in_name.into_iter().chain(in_extra).max()?;
    }
    Some(total / tokens.len() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(name: &str, q: &str) -> Option<i32> {
        score_token(name.as_bytes(), q.as_bytes())
    }

    #[test]
    fn tiers_rank_in_the_expected_order() {
        let exact = score("notepad", "notepad").unwrap();
        let prefix = score("notepad++", "note").unwrap();
        let word = score("sticky notes", "note").unwrap();
        let inside = score("keynote", "note").unwrap();
        let initials = score("visual studio code", "vsc").unwrap();
        assert!(exact > prefix && prefix > word && word > inside && inside > initials && initials > LOOSE);
        assert_eq!(score("network", "ntwk"), None, "letters with gaps aren't a match");
    }

    #[test]
    fn everyday_searches_still_land() {
        let find = |name: &str, typed: &str| score_all(&lower(name), "", &tokens(typed));
        assert_eq!(find("Visual Studio Code", "code"), Some(WORD_START));
        assert_eq!(find("Microsoft Edge", "edge"), Some(WORD_START));
        assert_eq!(find("Calculator", "calc"), Some(PREFIX));
        assert_eq!(find("Einstellungen", "einst"), Some(PREFIX));
        assert_eq!(find("Einstellungen", "EINST"), Some(PREFIX), "whatever the case");
        assert_eq!(find("Schriftgröße ändern", "änd"), Some(WORD_START), "umlauts are letters like any other");
        assert!(find("Schriftgröße ändern", "GRÖSSE").is_none() && find("Schriftgröße ändern", "GRÖßE").is_some());
        assert_eq!(find("Visual Studio Code", "vsc"), Some(INITIALS));
        assert!(find("Visual Studio Code", "visual code").is_some(), "every word is looked for on its own");
        assert!(find("Windows-Fotoanzeige", "wifi").is_none());
        assert!(find("Windows-Fotoanzeige", "wf").is_some() && find("Windows-Fotoanzeige", "foto").is_some());
    }

    #[test]
    fn only_whole_pieces_and_word_starts_match() {
        assert_eq!(score("windows-fotoanzeige", "wifi"), None, "wifi isn't hidden in Windows-Fotoanzeige");
        assert!(score("windows-fotoanzeige", "wf").is_some(), "the starts of its words are");
        assert!(score("visual studio code", "vsc").is_some());
        assert!(score("visual studio code", "dio").is_some(), "a piece of a word is");
        assert!(score("systeminformationen", "note").is_none());
        assert!(score("wi-fi settings", "wifi").is_none() && score("wi-fi settings", "wi-fi").is_some());
    }

    #[test]
    fn file_name_without_extension_counts_as_exact() {
        assert!(score("report.pdf", "report").unwrap() > score("report-final.pdf", "report").unwrap());
    }

    #[test]
    fn initials_find_multi_word_names() {
        assert!(score("visual studio code", "vsc").unwrap() >= INITIALS);
        assert_eq!(score("visual studio code", "xyz"), None);
    }

    #[test]
    fn scattered_letters_do_not_match() {
        assert_eq!(score("a very long name that happens to have letters xb", "ab"), None);
    }

    #[test]
    fn all_words_must_match() {
        let t = tokens("code studio");
        assert!(score_all("visual studio code", "", &t).is_some());
        let t = tokens("code banana");
        assert!(score_all("visual studio code", "", &t).is_none());
    }
}
