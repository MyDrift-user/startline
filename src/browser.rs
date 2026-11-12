//! Web searches go to the person's default browser and the search engine they chose there. Chromium browsers search
//! with their default engine when given "? words", Firefox and its forks with -search "words". A browser that isn't
//! one of those gets the search address instead.

use crate::platform::{self, wide};
use std::ffi::c_void;
use windows::Win32::System::Registry::*;
use windows::core::PCWSTR;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Family {
    Chromium,
    Firefox,
}

/// Program files of browsers built on Chromium.
const CHROMIUM: &[&str] = &["chrome.exe", "msedge.exe", "brave.exe", "vivaldi.exe", "opera.exe", "chromium.exe", "thorium.exe", "arc.exe", "iridium.exe", "cent.exe", "ungoogled-chromium.exe"]; // i18n-ignore: program file names
/// Program files of Firefox and its forks.
const FIREFOX: &[&str] = &["firefox.exe", "librewolf.exe", "waterfox.exe", "floorp.exe", "zen.exe", "mercury.exe", "firedragon.exe"]; // i18n-ignore: program file names

/// Which kind of browser the default one is, from its program and the name Windows registered it under.
pub fn family_of(prog_id: &str, exe: &str) -> Option<Family> {
    let file = exe.rsplit(['\\', '/']).next().unwrap_or(exe).to_ascii_lowercase();
    let path = exe.to_ascii_lowercase();
    let id = prog_id.to_ascii_lowercase();
    if CHROMIUM.contains(&file.as_str())
        // Opera and Opera GX start through a launcher; Yandex calls its program browser.exe.
        || (file == "launcher.exe" && path.contains("opera"))
        || (file == "browser.exe" && path.contains("yandex"))
    {
        return Some(Family::Chromium);
    }
    if FIREFOX.contains(&file.as_str()) {
        return Some(Family::Firefox);
    }
    // Known by the registered name when the program has an unusual file name.
    let chromium_ids = ["chromehtml", "chromebhtml", "chromedhtml", "chromiumhtm", "msedgehtm", "msedgebhtml", "msedgedhtml", "bravehtml", "vivaldihtm", "opera"]; // i18n-ignore: registered names
    let firefox_ids = ["firefoxurl", "librewolfhtm", "waterfoxurl", "floorpurl"]; // i18n-ignore: registered names
    if chromium_ids.iter().any(|known| id.starts_with(known)) {
        Some(Family::Chromium)
    } else if firefox_ids.iter().any(|known| id.starts_with(known)) {
        Some(Family::Firefox)
    } else {
        None
    }
}

/// The program from a registered open command, such as `"C:\...\chrome.exe" --single-argument %1`.
pub fn program_of_command(command: &str) -> Option<String> {
    let command = command.trim();
    if let Some(rest) = command.strip_prefix('"') {
        return rest.split_once('"').map(|(program, _)| program.to_string()).filter(|p| !p.is_empty());
    }
    let lower = command.to_ascii_lowercase();
    match lower.find(".exe") {
        Some(end) => Some(command[..end + 4].to_string()),
        None => command.split_whitespace().next().map(str::to_string),
    }
}

/// One argument written so the program reads it back exactly, quotes and backslashes included.
pub fn quote_arg(text: &str) -> String {
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in text.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

/// What to give the browser so it searches with its own default engine.
pub fn search_arguments(family: Family, query: &str) -> String {
    match family {
        Family::Chromium => quote_arg(&format!("? {query}")),
        // Firefox reads a word starting with - or / as an option of its own, wherever it stands; a space in front keeps
        // it the words to search for.
        Family::Firefox if query.starts_with(['-', '/']) => format!("-search {}", quote_arg(&format!(" {query}"))),
        Family::Firefox => format!("-search {}", quote_arg(query)),
    }
}

fn reg_text(root: HKEY, key: &str, name: Option<&str>) -> Option<String> {
    let key_w = wide(key);
    let name_w = name.map(wide);
    let name_ptr = name_w.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr()));
    let mut size = 0u32;
    // REG_EXPAND_SZ comes back with %ProgramFiles% and the like filled in.
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
    unsafe { RegGetValueW(root, PCWSTR(key_w.as_ptr()), name_ptr, flags, None, None, Some(&mut size)) }.ok().ok()?;
    let mut buffer = vec![0u16; (size as usize).div_ceil(2) + 1];
    unsafe { RegGetValueW(root, PCWSTR(key_w.as_ptr()), name_ptr, flags, None, Some(buffer.as_mut_ptr() as *mut c_void), Some(&mut size)) }.ok().ok()?;
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..len]))
}

/// The registered name Windows opens http addresses with. Windows itself is asked first: newer Windows 11 keeps the
/// choice under UserChoiceLatest and can leave an old one behind under UserChoice.
fn default_prog_id() -> Option<String> {
    use windows::Win32::UI::Shell::{ASSOCF_IS_PROTOCOL, ASSOCF_NOTRUNCATE, ASSOCSTR_PROGID, AssocQueryStringW};
    let protocol = wide("http"); // i18n-ignore: a protocol name
    let mut buffer = vec![0u16; 512];
    let mut size = buffer.len() as u32;
    let asked = unsafe {
        AssocQueryStringW(ASSOCF_IS_PROTOCOL | ASSOCF_NOTRUNCATE, ASSOCSTR_PROGID, PCWSTR(protocol.as_ptr()), PCWSTR::null(), Some(windows::core::PWSTR(buffer.as_mut_ptr())), &mut size)
    };
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    if asked.is_ok() && len > 0 {
        return Some(String::from_utf16_lossy(&buffer[..len]));
    }
    reg_text(HKEY_CURRENT_USER, "Software\\Microsoft\\Windows\\Shell\\Associations\\UrlAssociations\\http\\UserChoice", Some("ProgId"))
}

/// The default browser's registered name and program, as Windows opens http addresses with.
pub fn default_browser() -> Option<(String, String)> {
    let prog_id = default_prog_id()?;
    let command = reg_text(HKEY_CLASSES_ROOT, &format!("{prog_id}\\shell\\open\\command"), None)?;
    Some((prog_id.clone(), program_of_command(&command)?))
}

/// Searches the web for `query` in the default browser with its own engine, or opens `fallback` when the browser
/// isn't one that can be asked to.
pub fn search(query: String, fallback: String) {
    std::thread::spawn(move || {
        platform::com_init();
        let browser = default_browser().and_then(|(id, program)| family_of(&id, &program).map(|family| (family, program)));
        match browser {
            Some((family, program)) if platform::shell_open("", &program, &search_arguments(family, &query)) => {}
            _ => {
                platform::shell_open("", &fallback, "");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_browser_is_known_by_its_program_or_its_registered_name() {
        assert_eq!(family_of("ChromeHTML", r"C:\Program Files\Google\Chrome\Application\chrome.exe"), Some(Family::Chromium));
        assert_eq!(family_of("MSEdgeHTM", r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"), Some(Family::Chromium));
        assert_eq!(family_of("BraveHTML", r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe"), Some(Family::Chromium));
        assert_eq!(family_of("VivaldiHTM.ABC", r"C:\Users\me\AppData\Local\Vivaldi\Application\vivaldi.exe"), Some(Family::Chromium));
        assert_eq!(family_of("OperaStable", r"C:\Users\me\AppData\Local\Programs\Opera\launcher.exe"), Some(Family::Chromium));
        assert_eq!(family_of("FirefoxURL-308046B0AF4A39CB", r"C:\Program Files\Mozilla Firefox\firefox.exe"), Some(Family::Firefox));
        assert_eq!(family_of("LibreWolfHTM", r"C:\Program Files\LibreWolf\librewolf.exe"), Some(Family::Firefox));
        assert_eq!(family_of("ChromeHTML", r"C:\Odd\place\custom.exe"), Some(Family::Chromium), "the registered name is enough");
        assert_eq!(family_of("SomeOtherHTML", r"C:\Programs\other.exe"), None, "unknown: the search address is used");
        assert_eq!(family_of("SomeOtherHTML", r"C:\Tools\launcher.exe"), None, "a launcher that isn't Opera's");
    }

    #[test]
    fn the_program_is_read_from_the_open_command() {
        assert_eq!(program_of_command(r#""C:\Program Files\Google\Chrome\Application\chrome.exe" --single-argument %1"#).as_deref(), Some(r"C:\Program Files\Google\Chrome\Application\chrome.exe"));
        assert_eq!(program_of_command(r#""C:\Program Files\Mozilla Firefox\firefox.exe" -osint -url "%1""#).as_deref(), Some(r"C:\Program Files\Mozilla Firefox\firefox.exe"));
        assert_eq!(program_of_command(r"C:\Program Files\Browser\browser.exe %1").as_deref(), Some(r"C:\Program Files\Browser\browser.exe"));
        assert_eq!(program_of_command(r#""""#), None);
    }

    #[test]
    fn the_browser_gets_the_words_for_its_own_search_engine() {
        assert_eq!(search_arguments(Family::Chromium, "rust lang"), r#""? rust lang""#);
        assert_eq!(search_arguments(Family::Firefox, "rust lang"), r#"-search "rust lang""#);
        // Words that look like options stay words, in both kinds of browser.
        assert_eq!(search_arguments(Family::Firefox, "-ProfileManager"), r#"-search " -ProfileManager""#);
        assert_eq!(search_arguments(Family::Firefox, "/r/rust"), r#"-search " /r/rust""#);
        assert_eq!(search_arguments(Family::Chromium, "--incognito x"), r#""? --incognito x""#);
        // Quotes and a trailing backslash survive the trip through the command line.
        assert_eq!(quote_arg(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote_arg(r"C:\dir\"), r#""C:\dir\\""#);
        assert_eq!(quote_arg(r"a\b"), r#""a\b""#);
    }
}
