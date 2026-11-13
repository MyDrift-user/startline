//! Quick actions in search: the calculator, Windows' clipboard history, and switching the sound to another output.
//! The calculator answers within the key press; the clipboard and the sound devices are asked on a helper thread.

use crate::config::{Category, Config};
use crate::i18n::tr;
use crate::matching;
use crate::providers::{Ask, Provider};
use crate::search::{Icon, Row, RowKind};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Words that ask for the clipboard history itself, listing what's in it.
const CLIPBOARD_WORDS: [&str; 5] = ["clipboard", "zwischenablage", "appunti", "clip", "paste"]; // i18n-ignore: search words
/// Words that ask for the sound outputs.
const OUTPUT_WORDS: &str = "audio output sound speaker speakers headphones headset device lautsprecher kopfhörer ausgabe audioausgabe tonausgabe uscita altoparlanti cuffie"; // i18n-ignore: search words

/// A text in Windows' clipboard history.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub text: String,
}

/// A sound output.
#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub id: String,
    pub name: String,
    pub default: bool,
}

/// What the helper thread is asked.
enum Job {
    Search { generation: u64, tokens: Vec<String>, clipboard: bool, outputs: bool },
    UseOutput(String),
}

pub struct Quick {
    tx: Sender<Job>,
    generation: u64,
    done: Arc<Mutex<Option<(u64, Vec<Row>)>>>,
    /// The calculator's row, then the helper's.
    calc: Option<Row>,
    found: Vec<Row>,
}

impl Quick {
    pub fn start(ctx: egui::Context) -> Quick {
        let (tx, rx) = mpsc::channel::<Job>();
        let done: Arc<Mutex<Option<(u64, Vec<Row>)>>> = Arc::default();
        let slot = done.clone();
        let _ = std::thread::Builder::new().name("quick".into()).spawn(move || {
            crate::platform::com_init();
            let mut clips: Option<(Instant, Vec<Clip>)> = None;
            let mut outputs: Option<(Instant, Vec<Output>)> = None;
            loop {
                let mut job = match rx.recv_timeout(Duration::from_secs(60)) {
                    Ok(job) => job,
                    Err(RecvTimeoutError::Timeout) => {
                        // Nothing kept from the clipboard longer than needed.
                        clips = None;
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                };
                // Only the latest search matters.
                for newer in rx.try_iter() {
                    if let Job::UseOutput(id) = &job {
                        set_default_output(id);
                    }
                    job = newer;
                }
                match job {
                    Job::UseOutput(id) => {
                        set_default_output(&id);
                        outputs = None;
                    }
                    Job::Search { generation, tokens, clipboard, outputs: want_outputs } => {
                        let mut rows = Vec::new();
                        if want_outputs && wants_outputs(&tokens) {
                            if outputs.as_ref().is_none_or(|(at, _)| at.elapsed() > Duration::from_secs(5)) {
                                outputs = Some((Instant::now(), sound_outputs()));
                            }
                            rows.extend(output_rows(&tokens, &outputs.as_ref().unwrap().1));
                        }
                        if clipboard {
                            if clips.as_ref().is_none_or(|(at, _)| at.elapsed() > Duration::from_secs(2)) {
                                clips = Some((Instant::now(), clipboard_history()));
                            }
                            rows.extend(clip_rows(&tokens, &clips.as_ref().unwrap().1));
                        }
                        *slot.lock().unwrap() = Some((generation, rows));
                        ctx.request_repaint();
                    }
                }
            }
        });
        Quick { tx, generation: 0, done, calc: None, found: Vec::new() }
    }

    /// Makes another sound output the one Windows plays on.
    pub fn use_output(&self, id: String) {
        let _ = self.tx.send(Job::UseOutput(id));
    }
}

impl Provider for Quick {
    fn category(&self) -> Category {
        Category::Actions
    }

    fn ask(&mut self, ask: &Ask) {
        let shown = ask.cfg.shows(Category::Actions);
        self.calc = (shown && ask.cfg.quick_calculator).then(|| calc_row(ask.query)).flatten();
        self.generation += 1;
        let outputs = shown && ask.cfg.search_windows_settings && ask.policy.allows_row(&RowKind::Output(String::new()));
        let clipboard = shown && ask.cfg.quick_clipboard;
        if outputs || clipboard {
            let _ = self.tx.send(Job::Search { generation: self.generation, tokens: ask.tokens.to_vec(), clipboard, outputs });
        } else {
            self.found.clear();
        }
    }

    fn clear(&mut self) {
        self.generation += 1;
        self.calc = None;
        self.found.clear();
    }

    fn poll(&mut self) -> bool {
        match self.done.lock().unwrap().take() {
            Some((generation, rows)) if generation == self.generation => {
                self.found = rows;
                true
            }
            _ => false,
        }
    }

    fn rows(&self, _: &Config) -> Vec<Row> {
        self.calc.iter().cloned().chain(self.found.iter().cloned()).collect()
    }

    fn leads(&self) -> bool {
        // A sum is plainly what was asked for.
        self.calc.is_some()
    }
}

/// The calculator's answer as a row: Enter copies it.
pub fn calc_row(query: &str) -> Option<Row> {
    let answer = crate::calc::answer(query)?;
    Some(Row {
        category: Category::Actions,
        name: answer.shown,
        detail: crate::trf!("{kind}, Enter copies the result", kind = tr(answer.kind)),
        icon: Icon::Glyph('\u{E8EF}'),
        kind: RowKind::Copy(answer.copy),
        path: None,
    })
}

fn wants_outputs(tokens: &[String]) -> bool {
    !tokens.is_empty() && tokens.iter().any(|t| t.chars().count() >= 3 && OUTPUT_WORDS.split_whitespace().any(|w| w.starts_with(t.as_str())))
}

/// The outputs to switch to, the one in use last.
pub fn output_rows(tokens: &[String], outputs: &[Output]) -> Vec<Row> {
    if !wants_outputs(tokens) {
        return Vec::new();
    }
    // Words that aren't about outputs in general narrow down which device: "headphones" finds a headset by name.
    let narrowing: Vec<String> = tokens.iter().filter(|t| !OUTPUT_WORDS.split_whitespace().any(|w| w.starts_with(t.as_str()))).cloned().collect();
    let mut rows: Vec<Row> = outputs
        .iter()
        .filter(|o| narrowing.is_empty() || matching::score_all(&matching::lower(&o.name), "", &narrowing).is_some_and(|s| s > matching::LOOSE))
        .map(|o| Row {
            category: Category::Actions,
            name: crate::trf!("Play sound on {device}", device = o.name),
            detail: if o.default { tr("Sound output, in use now").to_string() } else { tr("Sound output").to_string() },
            icon: Icon::Glyph(if o.default { '\u{E767}' } else { '\u{E7F5}' }),
            kind: RowKind::Output(o.id.clone()),
            path: None,
        })
        .collect();
    rows.sort_by_key(|r| r.detail != tr("Sound output"));
    rows
}

/// Clipboard texts holding every word. A search that starts with "clipboard" or another of its words is about the
/// clipboard itself: it lists more, the latest first when nothing else is typed. Otherwise it takes three letters, and
/// at most three texts show, so the clipboard doesn't crowd an ordinary search.
pub fn clip_rows(tokens: &[String], clips: &[Clip]) -> Vec<Row> {
    let asked = tokens.first().is_some_and(|t| CLIPBOARD_WORDS.contains(&t.as_str()));
    let words: Vec<&String> = tokens.iter().skip(usize::from(asked)).collect();
    let typed: usize = words.iter().map(|w| w.chars().count()).sum::<usize>() + words.len().saturating_sub(1);
    if !asked && typed < 3 {
        return Vec::new();
    }
    clips
        .iter()
        .filter(|clip| {
            let lower = matching::lower(&clip.text);
            words.iter().all(|w| lower.contains(w.as_str()))
        })
        .take(if asked { 8 } else { 3 })
        .map(|clip| {
            let line: String = clip.text.split(['\r', '\n']).map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().chars().take(90).collect();
            Row {
                category: Category::Actions,
                name: line,
                detail: tr("Clipboard history, Enter copies it").to_string(),
                icon: Icon::Glyph('\u{E77F}'),
                kind: RowKind::Clip(clip.text.clone()),
                path: None,
            }
        })
        .collect()
}

/// What the quick actions find on this PC, for checking on a real machine. Only reads.
pub fn probe_report() -> String {
    let outputs = sound_outputs();
    let mut out = format!("sound outputs={}\n", outputs.len()); // i18n-ignore: probe report for tests
    for output in &outputs {
        out.push_str(&format!("  output default={} name={}\n", output.default, output.name)); // i18n-ignore: probe report for tests
    }
    // On a thread of its own like the helper's: the clipboard won't answer a thread that isn't single-threaded.
    let clipboard = std::thread::spawn(|| {
        crate::platform::com_init();
        let enabled = windows::ApplicationModel::DataTransfer::Clipboard::IsHistoryEnabled().map_or_else(|e| format!("{:?}", e.code()), |on| on.to_string());
        format!("clipboard history enabled={enabled} texts read={}\n", clipboard_history().len()) // i18n-ignore: probe report for tests
    });
    out.push_str(&clipboard.join().unwrap_or_default());
    out
}

// ---- Windows' clipboard history ----

/// The texts in Windows' clipboard history, newest first. Empty when the person hasn't switched it on in Windows.
fn clipboard_history() -> Vec<Clip> {
    use windows::ApplicationModel::DataTransfer::{Clipboard, ClipboardHistoryItemsResultStatus, StandardDataFormats};
    let read = || -> windows::core::Result<Vec<Clip>> {
        if !Clipboard::IsHistoryEnabled()? {
            return Ok(Vec::new());
        }
        let result = Clipboard::GetHistoryItemsAsync()?.join()?;
        if result.Status()? != ClipboardHistoryItemsResultStatus::Success {
            return Ok(Vec::new());
        }
        let text_format = StandardDataFormats::Text()?;
        let mut out = Vec::new();
        for item in result.Items()? {
            let view = item.Content()?;
            if view.Contains(&text_format)? {
                let text = view.GetTextAsync()?.join()?.to_string_lossy();
                if !text.trim().is_empty() {
                    out.push(Clip { text });
                }
            }
            if out.len() >= 25 {
                break;
            }
        }
        Ok(out)
    };
    read().unwrap_or_default()
}

// ---- Sound outputs ----

/// The sound outputs that are plugged in and on.
fn sound_outputs() -> Vec<Output> {
    use windows::Win32::Media::Audio::{DEVICE_STATE_ACTIVE, IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender};
    use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree, STGM_READ};
    use windows::Win32::Foundation::PROPERTYKEY;
    // The device's name as Windows shows it (PKEY_Device_FriendlyName).
    const FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY { fmtid: windows::core::GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0), pid: 14 };
    let read = || -> windows::core::Result<Vec<Output>> {
        unsafe {
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let id_of = |raw: windows::core::PWSTR| {
                let id = raw.to_string().unwrap_or_default();
                CoTaskMemFree(Some(raw.0 as *const std::ffi::c_void));
                id
            };
            let default = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).and_then(|d| d.GetId()).map(id_of).unwrap_or_default();
            let devices = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
            let mut out = Vec::new();
            for at in 0..devices.GetCount()? {
                let device = devices.Item(at)?;
                let id = id_of(device.GetId()?);
                let name = device.OpenPropertyStore(STGM_READ).and_then(|store| store.GetValue(&FRIENDLY_NAME)).map(|v| v.to_string()).unwrap_or_default();
                if !name.is_empty() {
                    out.push(Output { default: id == default, id, name });
                }
            }
            Ok(out)
        }
    };
    read().unwrap_or_default()
}

/// Makes Windows play everything on this output, for media, games and calls alike. Windows has no public way to do
/// this; its own sound settings use this interface, which every version since Windows 7 has.
fn set_default_output(id: &str) {
    use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
    use windows::core::{GUID, HRESULT, IUnknown, Interface, PCWSTR};
    const POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
    const POLICY_CONFIG: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);
    unsafe {
        let Ok(unknown) = CoCreateInstance::<_, IUnknown>(&POLICY_CONFIG_CLIENT, None, CLSCTX_ALL) else { return };
        let mut raw = std::ptr::null_mut();
        if unknown.query(&POLICY_CONFIG, &mut raw).is_err() || raw.is_null() {
            return;
        }
        // SetDefaultEndpoint comes thirteenth, after IUnknown's three and ten others.
        let table = *(raw as *const *const usize);
        let set: unsafe extern "system" fn(*mut std::ffi::c_void, PCWSTR, i32) -> HRESULT = std::mem::transmute(*table.add(13));
        let release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 = std::mem::transmute(*table.add(2));
        let wide = crate::platform::wide(id);
        // Console, multimedia and communications.
        for role in 0..3 {
            let _ = set(raw, PCWSTR(wide.as_ptr()), role);
        }
        release(raw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clipboard_is_searched_by_its_words() {
        let clips = vec![Clip { text: "Meeting notes\nfor Monday".into() }, Clip { text: "https://example.com/report".into() }, Clip { text: "   ".into() }];
        let names = |q: &str| clip_rows(&matching::tokens(q), &clips).into_iter().map(|r| r.name).collect::<Vec<_>>();
        assert_eq!(names("monday notes"), ["Meeting notes"], "the first line is shown, every word is looked for in all of it");
        assert_eq!(names("report"), ["https://example.com/report"]);
        assert_eq!(names("clipboard").len(), 3, "asked for itself, the latest are listed");
        assert!(names("x").is_empty() && names("no").is_empty(), "it takes three letters");
        assert_eq!(names("clipboard report"), ["https://example.com/report"], "after its word, the rest is looked for");
        assert_eq!(clip_rows(&matching::tokens("report"), &clips)[0].kind, RowKind::Clip("https://example.com/report".into()));
        let many: Vec<Clip> = (0..10).map(|i| Clip { text: format!("invoice {i}") }).collect();
        assert_eq!(clip_rows(&matching::tokens("invoice"), &many).len(), 3, "an ordinary search shows at most three");
        assert_eq!(clip_rows(&matching::tokens("clipboard invoice"), &many).len(), 8);
    }

    #[test]
    fn sound_outputs_show_when_asked_for() {
        let outputs = vec![
            Output { id: "a".into(), name: "Speakers (Realtek Audio)".into(), default: true },
            Output { id: "b".into(), name: "Headset (Jabra Evolve)".into(), default: false },
        ];
        let rows = |q: &str| output_rows(&matching::tokens(q), &outputs);
        assert!(rows("notepad").is_empty());
        let all = rows("audio output");
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].kind, RowKind::Output("b".into()), "the one to switch to comes first");
        assert_eq!(all[1].detail, "Sound output, in use now");
        let jabra = rows("sound jabra");
        assert_eq!(jabra.len(), 1);
        assert_eq!(jabra[0].name, "Play sound on Headset (Jabra Evolve)");
    }

    #[test]
    fn the_calculator_row_copies_its_answer() {
        let row = calc_row("12 * 3").unwrap();
        assert_eq!(row.name, "= 36");
        assert_eq!(row.kind, RowKind::Copy("36".into()));
        assert!(calc_row("budget 2026").is_none());
    }
}
