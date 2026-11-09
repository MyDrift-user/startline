//! The file index: every name lives in one long block of memory, so looking through
//! a million files is a single fast scan instead of a million small lookups. It is saved
//! to disk so search works from the first moment, and kept fresh by watching for changes.

use crate::matching;
use crate::platform;
use memchr::memmem;
use rayon::prelude::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

#[path = "index_manager.rs"]
mod manager;
pub use manager::*;

const FLAG_DIR: u8 = 1;
const FLAG_ROOT: u8 = 2;
const FLAG_DEAD: u8 = 4;
const NO_PARENT: u32 = u32::MAX;

#[repr(C)]
#[derive(Clone, Copy)]
struct Entry {
    lower_off: u32,
    orig_off: u32,
    parent: u32,
    mtime: u32,
    /// Bytes, for files.
    size: u64,
    lower_len: u16,
    orig_len: u16,
    flags: u8,
    depth: u8,
    _pad: u16,
}

#[derive(Default)]
pub struct Index {
    entries: Vec<Entry>,
    /// Lowercase names, each followed by a zero so a match can't run over two names.
    lower: Vec<u8>,
    orig: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub mtime: u32,
    pub size: u64,
    pub score: i32,
}

impl Index {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    fn push(&mut self, name: &str, parent: u32, depth: u8, mtime: u32, flags: u8) -> u32 {
        self.push_sized(name, parent, depth, mtime, 0, flags)
    }

    fn push_sized(&mut self, name: &str, parent: u32, depth: u8, mtime: u32, size: u64, flags: u8) -> u32 {
        let name_bytes = &name.as_bytes()[..name.len().min(u16::MAX as usize - 1)];
        let lowered = matching::lower(name);
        let lower_bytes = &lowered.as_bytes()[..lowered.len().min(u16::MAX as usize - 1)];
        let entry = Entry {
            lower_off: self.lower.len() as u32,
            orig_off: self.orig.len() as u32,
            parent,
            mtime,
            size,
            lower_len: lower_bytes.len() as u16,
            orig_len: name_bytes.len() as u16,
            flags,
            depth,
            _pad: 0,
        };
        self.lower.extend_from_slice(lower_bytes);
        self.lower.push(0);
        self.orig.extend_from_slice(name_bytes);
        self.entries.push(entry);
        (self.entries.len() - 1) as u32
    }

    fn lower_name(&self, e: &Entry) -> &[u8] {
        &self.lower[e.lower_off as usize..e.lower_off as usize + e.lower_len as usize]
    }

    fn orig_name(&self, e: &Entry) -> &str {
        let bytes = &self.orig[e.orig_off as usize..e.orig_off as usize + e.orig_len as usize];
        std::str::from_utf8(bytes).unwrap_or("")
    }

    pub fn full_path(&self, idx: usize) -> String {
        let mut parts: Vec<&str> = Vec::with_capacity(12);
        let mut cur = idx as u32;
        while cur != NO_PARENT {
            let e = &self.entries[cur as usize];
            parts.push(self.orig_name(e));
            cur = e.parent;
        }
        let mut out = String::with_capacity(parts.iter().map(|p| p.len() + 1).sum());
        for part in parts.iter().rev() {
            if !out.is_empty() && !out.ends_with('\\') {
                out.push('\\');
            }
            out.push_str(part);
        }
        out
    }

    fn lower_path_contains(&self, idx: usize, token: &[u8]) -> bool {
        let mut cur = idx as u32;
        while cur != NO_PARENT {
            let e = &self.entries[cur as usize];
            if memmem::find(self.lower_name(e), token).is_some() {
                return true;
            }
            cur = e.parent;
        }
        false
    }

    fn is_alive(&self, idx: usize) -> bool {
        let mut cur = idx as u32;
        while cur != NO_PARENT {
            let e = &self.entries[cur as usize];
            if e.flags & FLAG_DEAD != 0 {
                return false;
            }
            cur = e.parent;
        }
        true
    }

    /// Finds an entry by its full path. Used when Windows reports that a file changed.
    fn find_path(&self, path: &str) -> Option<usize> {
        let lowered = matching::lower(path.trim_end_matches('\\'));
        let last = lowered.rsplit('\\').next()?;
        for pos in memmem::find_iter(&self.lower, last.as_bytes()) {
            let idx = self.entries.partition_point(|e| (e.lower_off as usize) <= pos);
            if idx == 0 {
                continue;
            }
            let e = &self.entries[idx - 1];
            if (e.lower_off as usize) != pos || e.lower_len as usize != last.len() {
                continue;
            }
            if e.flags & FLAG_DEAD == 0 && matching::lower(&self.full_path(idx - 1)).trim_end_matches('\\') == lowered {
                return Some(idx - 1);
            }
        }
        // A root's stored name is its whole path.
        self.entries.iter().position(|e| {
            e.flags & FLAG_ROOT != 0 && self.lower_name(e).strip_suffix(b"\\").unwrap_or(self.lower_name(e)) == lowered.as_bytes()
        })
    }

    fn append_listing(&mut self, parent: u32, depth: u8, listing: &Listing) {
        let me = self.push(&listing.name, parent, depth, listing.mtime, FLAG_DIR);
        self.append_children(me, depth.saturating_add(1), listing);
    }

    fn append_children(&mut self, me: u32, depth: u8, listing: &Listing) {
        for (name, mtime, size) in &listing.files {
            self.push_sized(name, me, depth, *mtime, *size, 0);
        }
        for dir in &listing.dirs {
            self.append_listing(me, depth, dir);
        }
    }

    /// Looks through all names in parallel and returns the best `limit` matches.
    #[cfg(test)]
    pub fn search(&self, query: &str, limit: usize, cancel: &dyn Fn() -> bool) -> Vec<Hit> {
        self.search_filtered(query, &FileFilter::default(), limit, cancel).hits
    }

    /// The best matches that pass the filter, and a count of every file type the words matched,
    /// filter or not, so the filter buttons can show what else there is.
    pub fn search_filtered(&self, query: &str, filter: &FileFilter, limit: usize, cancel: &dyn Fn() -> bool) -> Answer {
        let parsed = if filter.regex { Query::regex(query) } else { Query::parse(query) };
        if self.entries.is_empty() || parsed.is_empty() {
            return Answer { error: parsed.error, ..Default::default() };
        }
        let mut wanted = filter.clone();
        wanted.exts.extend(parsed.typed.iter().cloned());
        let mode = if parsed.needle.is_empty() { Mode::Browse } else { Mode::Direct };
        let (mut hits, mut facets) = self.scan(&parsed, &wanted, limit, cancel, mode);
        // Nothing direct: allow letters with gaps, so small typos and shorthand still land.
        if facets.total == 0 && parsed.needle.len() >= 3 && !cancel() {
            (hits, facets) = self.scan(&parsed, &wanted, limit, cancel, Mode::Fuzzy);
        }
        hits.sort_unstable_by_key(|h| std::cmp::Reverse(h.1));
        hits.truncate(limit);
        let hits = hits
            .into_iter()
            .map(|(idx, score)| {
                let e = &self.entries[idx];
                Hit {
                    path: self.full_path(idx),
                    name: self.orig_name(e).to_string(),
                    is_dir: e.flags & FLAG_DIR != 0,
                    mtime: e.mtime,
                    size: e.size,
                    score,
                }
            })
            .collect();
        Answer { hits, facets, error: parsed.error }
    }

    fn scan(&self, q: &Query, filter: &FileFilter, limit: usize, cancel: &dyn Fn() -> bool, mode: Mode) -> (Vec<(usize, i32)>, Facets) {
        let total = self.entries.len();
        let chunk = (total / (rayon::current_num_threads() * 4).max(1)).max(4096);
        let ranges: Vec<_> = (0..total.div_ceil(chunk)).map(|i| i * chunk..((i + 1) * chunk).min(total)).collect();
        let finder = memmem::Finder::new(q.needle.as_bytes());
        let cancel = SyncCancel(cancel);
        let now = platform::unix_now();

        let per_chunk: Vec<(Vec<(usize, i32)>, Tally)> = ranges
            .into_par_iter()
            .map(|range| {
                let mut out: Vec<(usize, i32)> = Vec::new();
                let mut tally = Tally::default();
                let mut visit = |idx: usize| {
                    if let Some(score) = self.score_entry(idx, q, now, mode) {
                        let e = &self.entries[idx];
                        let (name, is_dir) = (self.lower_name(e), e.flags & FLAG_DIR != 0);
                        tally.add(name, is_dir);
                        if !filter.passes(name, is_dir) {
                            return;
                        }
                        out.push((idx, score));
                        if out.len() >= limit * 8 + 64 {
                            out.select_nth_unstable_by_key(limit, |h| std::cmp::Reverse(h.1));
                            out.truncate(limit);
                        }
                    }
                };
                if cancel.get() {
                    return (Vec::new(), Tally::default());
                }
                if mode == Mode::Direct {
                    self.scan_range(range, &finder, &mut visit);
                } else {
                    for idx in range {
                        visit(idx);
                    }
                }
                (out, tally)
            })
            .collect();

        let mut merged: Vec<(usize, i32)> = Vec::new();
        let mut tally = Tally::default();
        for (out, part) in per_chunk {
            merged.extend(out);
            tally.merge(part);
        }
        if merged.len() > limit {
            merged.select_nth_unstable_by_key(limit, |h| std::cmp::Reverse(h.1));
            merged.truncate(limit);
        }
        (merged, tally.finish())
    }

    /// How many files and folders each searched place currently holds.
    pub fn counts_by_root(&self) -> Vec<(String, usize)> {
        let mut root_of = vec![NO_PARENT; self.entries.len()];
        let mut counts: Vec<(u32, usize)> = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            if e.flags & FLAG_ROOT != 0 {
                root_of[i] = i as u32;
                counts.push((i as u32, 0));
            } else if e.parent != NO_PARENT {
                root_of[i] = root_of[e.parent as usize];
                if let Some(slot) = counts.iter_mut().find(|c| c.0 == root_of[i]) {
                    slot.1 += 1;
                }
            }
        }
        counts.into_iter().map(|(root, count)| (self.orig_name(&self.entries[root as usize]).to_string(), count)).collect()
    }

    /// The core loop: one substring scan over a block of names. A found position maps back
    /// to its entry with a short gallop from the last one, keeping common letters cheap too.
    fn scan_range(&self, range: std::ops::Range<usize>, finder: &memmem::Finder, visit: &mut impl FnMut(usize)) {
        let start = self.entries[range.start].lower_off as usize;
        let end = if range.end < self.entries.len() { self.entries[range.end].lower_off as usize } else { self.lower.len() };
        let hay = &self.lower[start..end];
        let mut at = 0;
        let mut idx = range.start;
        while at < hay.len() {
            let Some(found) = finder.find(&hay[at..]) else { break };
            let abs = start + at + found;
            // Gallop forward to the entry that holds this position.
            let mut step = 1;
            while idx + step < range.end && (self.entries[idx + step].lower_off as usize) <= abs {
                idx += step;
                step *= 2;
            }
            while step > 0 {
                if idx + step < range.end && (self.entries[idx + step].lower_off as usize) <= abs {
                    idx += step;
                }
                step /= 2;
            }
            visit(idx);
            let e = &self.entries[idx];
            at = e.lower_off as usize + e.lower_len as usize + 1 - start;
            idx += 1;
            if idx >= range.end {
                break;
            }
        }
    }

    fn score_entry(&self, idx: usize, q: &Query, now: u32, mode: Mode) -> Option<i32> {
        let e = &self.entries[idx];
        if e.flags & (FLAG_ROOT | FLAG_DEAD) != 0 {
            return None;
        }
        let name = self.lower_name(e);
        if !q.terms.is_empty() && !q.passes(name, e.flags & FLAG_DIR != 0, e.size, e.mtime) {
            return None;
        }
        let mut score = match mode {
            Mode::Fuzzy => matching::subsequence_score(name, q.needle.as_bytes())?,
            Mode::Direct => matching::score_token(name, q.needle.as_bytes())?,
            // Only a file type was typed: every name qualifies and the filter picks.
            Mode::Browse => 300,
        };
        for token in &q.others {
            match matching::score_token(name, token.as_bytes()) {
                Some(s) if s > 200 => score += s / 4,
                // Other words may name a parent folder: "taxes 2025" inside "2025\taxes.pdf".
                _ if self.lower_path_contains(idx, token.as_bytes()) => score += 40,
                _ => return None,
            }
        }
        score -= e.depth as i32 * 6;
        score -= (e.lower_len as i32).min(120) / 2;
        let age = now.saturating_sub(e.mtime);
        if age < 86_400 * 7 {
            score += 70;
        } else if age < 86_400 * 60 {
            score += 35;
        }
        if e.flags & FLAG_DIR != 0 {
            score += 15;
        } else if name.ends_with(b".dll") || name.ends_with(b".sys") || name.ends_with(b".tmp") || name.ends_with(b".log") {
            score -= 60;
        }
        if !self.is_alive(idx) {
            return None;
        }
        Some(score)
    }

    // ---- Places within the index ----

    /// The searched places it holds, as their full paths.
    pub fn root_names(&self) -> Vec<String> {
        self.entries.iter().filter(|e| e.flags & FLAG_ROOT != 0 && e.flags & FLAG_DEAD == 0).map(|e| self.orig_name(e).to_string()).collect()
    }

    /// A copy holding only the places `keep` says yes to, with all they hold. Removed items are left behind.
    pub fn copy_roots(&self, keep: &dyn Fn(&str) -> bool) -> Index {
        let mut out = Index::default();
        out.append_roots(self, keep);
        out
    }

    /// Adds the places of `other` that `keep` says yes to, with all they hold.
    pub fn append_roots(&mut self, other: &Index, keep: &dyn Fn(&str) -> bool) {
        // Every entry comes after its folder, so one pass in order finds each one's new folder.
        let mut moved = vec![NO_PARENT; other.entries.len()];
        for (i, e) in other.entries.iter().enumerate() {
            if e.flags & FLAG_DEAD != 0 {
                continue;
            }
            let parent = if e.flags & FLAG_ROOT != 0 {
                if !keep(other.orig_name(e)) {
                    continue;
                }
                NO_PARENT
            } else if e.parent == NO_PARENT || moved[e.parent as usize] == NO_PARENT {
                continue;
            } else {
                moved[e.parent as usize]
            };
            moved[i] = self.push_sized(other.orig_name(e), parent, e.depth, e.mtime, e.size, e.flags);
        }
    }

    // ---- Saving and loading ----

    fn save(&self, fingerprint: u64) -> std::io::Result<()> {
        use std::io::Write;
        let path = cache_path();
        let tmp = path.with_extension("tmp");
        let mut file = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        for value in [CACHE_MAGIC, fingerprint, self.entries.len() as u64, self.lower.len() as u64, self.orig.len() as u64] {
            file.write_all(&value.to_le_bytes())?;
        }
        // Entry is a plain block of numbers with no gaps, written exactly as it sits in memory.
        let raw = unsafe {
            std::slice::from_raw_parts(self.entries.as_ptr() as *const u8, self.entries.len() * std::mem::size_of::<Entry>())
        };
        file.write_all(raw)?;
        file.write_all(&self.lower)?;
        file.write_all(&self.orig)?;
        file.flush()?;
        drop(file);
        std::fs::rename(tmp, path)
    }

    /// The saved index, and the fingerprint of the rules it was read under.
    fn load() -> Option<(Index, u64)> {
        let bytes = std::fs::read(cache_path()).ok()?;
        let word = |i: usize| -> Option<u64> { Some(u64::from_le_bytes(bytes.get(i * 8..i * 8 + 8)?.try_into().ok()?)) };
        if word(0)? != CACHE_MAGIC {
            return None;
        }
        let fingerprint = word(1)?;
        let (count, lower_len, orig_len) = (word(2)? as usize, word(3)? as usize, word(4)? as usize);
        let entries_len = count.checked_mul(std::mem::size_of::<Entry>())?;
        let lower_at = 40usize.checked_add(entries_len)?;
        let orig_at = lower_at.checked_add(lower_len)?;
        if bytes.len() != orig_at.checked_add(orig_len)? {
            return None;
        }
        let mut entries: Vec<Entry> = Vec::with_capacity(count);
        unsafe {
            std::ptr::copy_nonoverlapping(bytes[40..lower_at].as_ptr(), entries.as_mut_ptr() as *mut u8, entries_len);
            entries.set_len(count);
        }
        let lower = bytes[lower_at..orig_at].to_vec();
        let orig = bytes[orig_at..].to_vec();
        // A damaged cache must never index out of bounds later on.
        let sound = entries.iter().enumerate().all(|(i, e)| {
            (e.lower_off as usize + e.lower_len as usize) < lower.len().max(1)
                && (e.orig_off as usize + e.orig_len as usize) <= orig.len()
                && (e.parent == NO_PARENT || (e.parent as usize) < i)
        });
        sound.then_some((Index { entries, lower, orig }, fingerprint))
    }
}

// Raised whenever Entry changes, so an older saved index is read again rather than misread.
const CACHE_MAGIC: u64 = 0x5354_4c49_4e45_3032;

fn cache_path() -> PathBuf {
    crate::config::data_dir().join("index.bin")
}

struct SyncCancel<'a>(&'a dyn Fn() -> bool);
// The check only reads an atomic counter, so sharing it between threads is safe.
unsafe impl Sync for SyncCancel<'_> {}
impl SyncCancel<'_> {
    fn get(&self) -> bool {
        (self.0)()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Direct,
    Fuzzy,
    Browse,
}

/// Which kinds of result to keep. Empty keeps everything.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileFilter {
    /// With the dot, lowercase: ".pdf".
    pub exts: Vec<String>,
    pub folders: bool,
    /// The whole search is one regular expression, from the switch in the search box.
    pub regex: bool,
}

impl FileFilter {
    /// Whether it narrows the results at all. Regex mode changes how words are read, not what kinds are kept.
    pub fn is_empty(&self) -> bool {
        self.exts.is_empty() && !self.folders
    }

    fn passes(&self, lower_name: &[u8], is_dir: bool) -> bool {
        if self.is_empty() {
            return true;
        }
        if is_dir {
            return self.folders;
        }
        self.exts.iter().any(|ext| lower_name.ends_with(ext.as_bytes()))
    }
}

/// How many matches there were of each kind.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facets {
    /// Most common first.
    pub exts: Vec<(String, u32)>,
    pub folders: u32,
    pub total: u32,
}

#[derive(Default)]
pub struct Answer {
    pub hits: Vec<Hit>,
    pub facets: Facets,
    /// Why a regular expression in the search couldn't be used.
    pub error: Option<String>,
}

#[derive(Default)]
struct Tally {
    /// A file type's letters packed into a number, so counting needs no text copies.
    exts: std::collections::HashMap<u128, u32>,
    folders: u32,
    total: u32,
}

impl Tally {
    fn add(&mut self, lower_name: &[u8], is_dir: bool) {
        self.total += 1;
        if is_dir {
            self.folders += 1;
            return;
        }
        let Some(dot) = lower_name.iter().rposition(|&b| b == b'.') else { return };
        let ext = &lower_name[dot..];
        // Not a file type: "report.final draft", "v1.2", a name that only starts with a dot.
        if dot == 0 || !(2..=12).contains(&ext.len()) || !ext[1..].iter().any(|b| b.is_ascii_alphabetic()) || ext.contains(&b' ') {
            return;
        }
        let mut packed = [0u8; 16];
        packed[..ext.len()].copy_from_slice(ext);
        *self.exts.entry(u128::from_le_bytes(packed)).or_default() += 1;
    }

    fn merge(&mut self, other: Tally) {
        for (ext, count) in other.exts {
            *self.exts.entry(ext).or_default() += count;
        }
        self.folders += other.folders;
        self.total += other.total;
    }

    fn finish(self) -> Facets {
        let mut exts: Vec<(String, u32)> = self
            .exts
            .into_iter()
            .map(|(packed, count)| {
                let bytes = packed.to_le_bytes();
                let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
                (String::from_utf8_lossy(&bytes[..len]).into_owned(), count)
            })
            .collect();
        exts.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Facets { exts, folders: self.folders, total: self.total }
    }
}

/// A word that is really a file type: ".pdf", "*.pdf" or "ext:pdf". Returned as ".pdf".
pub fn typed_extension(word: &str) -> Option<String> {
    let rest = word.strip_prefix("ext:").or_else(|| word.strip_prefix("*.")).or_else(|| word.strip_prefix('.'))?;
    let rest = rest.trim_start_matches('.');
    let looks_right = !rest.is_empty() && rest.len() <= 11 && rest.bytes().all(|b| b.is_ascii_alphanumeric());
    looks_right.then(|| format!(".{}", rest.to_ascii_lowercase()))
}

/// What a file or folder has to be, besides its name holding the words.
#[derive(Clone, Debug)]
enum Term {
    /// The name doesn't hold this.
    Without(String),
    /// "exact text": the name holds this, spaces and all.
    Phrase(String),
    /// Wildcards: `*` any letters, `?` one. Matched against the whole name.
    Glob(Vec<u8>, bool),
    Kind(Kind),
    SizeMin(u64),
    SizeMax(u64),
    After(u32),
    Before(u32),
    Regex(regex::bytes::Regex, bool),
    /// A regular expression that isn't valid: finds nothing, and the person is told why.
    Never,
}

/// The kinds `kind:` knows, and the words for them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Folder,
    Document,
    Picture,
    Video,
    Music,
    Archive,
    Code,
    App,
}

impl Kind {
    fn from_word(word: &str) -> Option<Kind> {
        Some(match word {
            "folder" | "folders" | "dir" => Kind::Folder,
            "doc" | "docs" | "document" | "documents" | "text" => Kind::Document,
            "image" | "images" | "picture" | "pictures" | "photo" | "photos" => Kind::Picture,
            "video" | "videos" | "movie" | "movies" => Kind::Video,
            "audio" | "music" | "sound" | "song" | "songs" => Kind::Music,
            "archive" | "archives" | "zip" => Kind::Archive,
            "code" | "source" => Kind::Code,
            "app" | "apps" | "program" | "programs" | "exe" => Kind::App,
            _ => return None,
        })
    }

    fn types(self) -> &'static [&'static str] {
        match self {
            Kind::Folder => &[],
            Kind::Document => crate::search::FILE_KINDS[0].1,
            Kind::Picture => crate::search::FILE_KINDS[1].1,
            Kind::Video => crate::search::FILE_KINDS[2].1,
            Kind::Music => crate::search::FILE_KINDS[3].1,
            Kind::Archive => crate::search::FILE_KINDS[4].1,
            Kind::Code => &[".rs", ".py", ".js", ".ts", ".tsx", ".jsx", ".c", ".h", ".cpp", ".hpp", ".cs", ".java", ".go", ".rb", ".php", ".ps1", ".sh", ".bat", ".cmd", ".json", ".toml", ".yaml", ".yml", ".xml", ".html", ".css", ".sql", ".kt", ".swift", ".lua"],
            Kind::App => &[".exe", ".msi", ".lnk", ".appx", ".msix", ".url"],
        }
    }

    fn holds(self, lower_name: &[u8], is_dir: bool) -> bool {
        match self {
            Kind::Folder => is_dir,
            _ => !is_dir && self.types().iter().any(|t| lower_name.ends_with(t.as_bytes())),
        }
    }
}

/// A search, as typed.
///
/// `report final`: names holding both words, or the other words naming a folder it's in; `"exact text"`;
/// `*.rs`, `img_??.png`: wildcards; `-draft`: without; `ext:pdf,docx` (or `.pdf`); `kind:picture` and
/// folder, document, video, music, archive, code, app; `size:>10mb`, `size:<1k`, `size:1mb..1gb`;
/// `modified:today`, yesterday, week, month, year, `>2026-01-01`, `<2026-01-01`, 2025, 2025-03 (`date:` the same);
/// `re:^img_\d+` or `/^img_\d+/`: a regular expression, whatever the case.
#[derive(Clone, Debug, Default)]
struct Query {
    /// The longest word: the rarest, so it drives the scan.
    needle: String,
    others: Vec<String>,
    typed: Vec<String>,
    terms: Vec<Term>,
    /// Why a regular expression couldn't be used.
    error: Option<String>,
}

/// Words, with "quoted text" kept together, quotes and all.
fn split_words(query: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    for c in query.chars() {
        match c {
            '"' => {
                word.push(c);
                quoted = !quoted;
            }
            c if c.is_whitespace() && !quoted => {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

/// `re:pattern`, `regex:pattern` (quotes allowed) or `/pattern/`.
fn regex_word(word: &str) -> Option<&str> {
    let lower = word.to_ascii_lowercase();
    for prefix in ["re:", "regex:"] {
        if lower.starts_with(prefix) {
            return Some(word[prefix.len()..].trim_matches('"')).filter(|s| !s.is_empty());
        }
    }
    (word.len() >= 3 && word.starts_with('/') && word.ends_with('/')).then(|| &word[1..word.len() - 1])
}

/// Wildcards, but not a question mark ending a question: "what is my ip?" is words.
fn is_wildcard(word: &str) -> bool {
    word.contains('*') || word.trim_end_matches('?').contains('?')
}

/// Whether the words use any of the search language beyond plain words, which only files can answer.
pub fn uses_file_syntax(query: &str) -> bool {
    split_words(query).iter().any(|word| {
        let lower = matching::lower(word.trim_start_matches('-'));
        regex_word(word.trim_start_matches('-')).is_some()
            || is_wildcard(&lower)
            || lower.split_once(':').is_some_and(|(key, _)| matches!(key, "ext" | "type" | "kind" | "is" | "size" | "modified" | "date" | "mod"))
    })
}

fn parse_size(text: &str) -> Option<u64> {
    let text = text.trim();
    let split = text.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',')).unwrap_or(text.len());
    let number: f64 = text[..split].replace(',', ".").parse().ok()?;
    let unit = match text[split..].trim() {
        "" | "b" => 1.0,
        "k" | "kb" => 1024.0,
        "m" | "mb" => 1024.0 * 1024.0,
        "g" | "gb" => 1024.0f64.powi(3),
        "t" | "tb" => 1024.0f64.powi(4),
        _ => return None,
    };
    Some((number * unit) as u64)
}

/// A day typed as 2026-03-01, 2026-03 or 2026, or as 01.03.2026 and 01/03/2026: its first and last second.
fn parse_day(text: &str) -> Option<(u32, u32)> {
    let parts: Vec<&str> = text.split(['-', '.', '/']).collect();
    let number = |s: &str| s.parse::<u32>().ok();
    let (year, month, day, whole) = match parts.as_slice() {
        [y] if y.len() == 4 => (number(y)?, 1, 1, "year"),
        [y, m] if y.len() == 4 => (number(y)?, number(m)?, 1, "month"),
        [y, m, d] if y.len() == 4 => (number(y)?, number(m)?, number(d)?, "day"),
        [d, m, y] if y.len() == 4 => (number(y)?, number(m)?, number(d)?, "day"),
        _ => return None,
    };
    let start = platform::local_midnight(year as i32, month, day)?;
    let end = match whole {
        "year" => platform::local_midnight(year as i32 + 1, 1, 1)?,
        "month" if month == 12 => platform::local_midnight(year as i32 + 1, 1, 1)?,
        "month" => platform::local_midnight(year as i32, month + 1, 1)?,
        _ => start + 86_400,
    };
    Some((start.min(u32::MAX as u64) as u32, end.min(u32::MAX as u64) as u32))
}

fn parse_date(value: &str) -> Option<Vec<Term>> {
    let now = platform::unix_now();
    let today = platform::local_time(now as u64);
    let midnight = platform::local_midnight(today.year, today.month, today.day)? as u32;
    const DAY: u32 = 86_400;
    Some(match value {
        "today" => vec![Term::After(midnight)],
        "yesterday" => vec![Term::After(midnight.saturating_sub(DAY)), Term::Before(midnight)],
        "week" | "thisweek" => vec![Term::After(now.saturating_sub(7 * DAY))],
        "month" | "thismonth" => vec![Term::After(now.saturating_sub(30 * DAY))],
        "year" | "thisyear" => vec![Term::After(now.saturating_sub(365 * DAY))],
        _ => {
            if let Some(rest) = value.strip_prefix(">=").or_else(|| value.strip_prefix('>')) {
                vec![Term::After(parse_day(rest)?.0)]
            } else if let Some(rest) = value.strip_prefix("<=") {
                vec![Term::Before(parse_day(rest)?.1)]
            } else if let Some(rest) = value.strip_prefix('<') {
                vec![Term::Before(parse_day(rest)?.0)]
            } else if let Some((from, to)) = value.split_once("..") {
                vec![Term::After(parse_day(from)?.0), Term::Before(parse_day(to)?.1)]
            } else {
                let (start, end) = parse_day(value)?;
                vec![Term::After(start), Term::Before(end)]
            }
        }
    })
}

/// `key:value` as terms, or `None` if it isn't one the search language knows.
fn parse_filter(key: &str, value: &str, typed: &mut Vec<String>) -> Option<Vec<Term>> {
    Some(match key {
        "ext" | "type" => {
            typed.extend(value.split([',', '|', ';']).map(|t| t.trim().trim_start_matches("*.").trim_start_matches('.')).filter(|t| !t.is_empty()).map(|t| format!(".{t}")));
            Vec::new()
        }
        "kind" | "is" => vec![Term::Kind(Kind::from_word(value)?)],
        "size" => {
            if let Some((from, to)) = value.split_once("..") {
                vec![Term::SizeMin(parse_size(from)?), Term::SizeMax(parse_size(to)?)]
            } else if let Some(rest) = value.strip_prefix(">=").or_else(|| value.strip_prefix('>')) {
                vec![Term::SizeMin(parse_size(rest)?)]
            } else if let Some(rest) = value.strip_prefix("<=").or_else(|| value.strip_prefix('<')) {
                vec![Term::SizeMax(parse_size(rest)?)]
            } else {
                vec![Term::SizeMin(parse_size(value)?)]
            }
        }
        "modified" | "date" | "mod" => parse_date(value)?,
        _ => return None,
    })
}

/// A wildcard pattern, `*` and `?`, against the whole name.
fn glob_match(pattern: &[u8], name: &[u8]) -> bool {
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == name[n]) {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some((p, n));
            p += 1;
        } else if let Some((sp, sn)) = star {
            p = sp + 1;
            n = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == b'*')
}

/// The line of a regular expression's error that says what's wrong.
fn short_error(error: &regex::Error) -> String {
    let text = error.to_string();
    text.lines().filter(|l| !l.trim().is_empty()).last().unwrap_or("").trim().trim_start_matches("error: ").to_string()
}

impl Query {
    fn parse(query: &str) -> Query {
        let mut out = Query::default();
        let mut words = Vec::new();
        for raw in split_words(query) {
            let (without, word) = match raw.strip_prefix('-') {
                Some(rest) if !rest.is_empty() => (true, rest.to_string()),
                _ => (false, raw.clone()),
            };
            if let Some(pattern) = regex_word(&word) {
                out.push_regex(pattern, without);
                continue;
            }
            let lower = matching::lower(&word);
            if !without {
                if let Some(ext) = typed_extension(&lower) {
                    out.typed.push(ext);
                    continue;
                }
            }
            if let Some((key, value)) = lower.split_once(':') {
                if let Some(terms) = parse_filter(key, value, &mut out.typed) {
                    out.terms.extend(terms);
                    continue;
                }
            }
            if lower.starts_with('"') {
                let inner = lower.trim_matches('"').to_string();
                if inner.is_empty() {
                    continue;
                }
                out.terms.push(if without { Term::Without(inner) } else { Term::Phrase(inner) });
            } else if is_wildcard(&lower) {
                out.terms.push(Term::Glob(lower.into_bytes(), without));
            } else if without {
                out.terms.push(Term::Without(lower));
            } else if !lower.is_empty() {
                words.push(lower);
            }
        }
        // A quoted phrase can drive the scan too when nothing else is typed.
        if words.is_empty() {
            if let Some(Term::Phrase(phrase)) = out.terms.iter().find(|t| matches!(t, Term::Phrase(_))) {
                words.push(phrase.clone());
            }
        }
        let longest = words.iter().enumerate().max_by_key(|(_, w)| w.len()).map(|(i, _)| i);
        if let Some(i) = longest {
            out.needle = words.remove(i);
        }
        out.others = words;
        out
    }

    /// Regex mode: the whole search is one regular expression.
    fn regex(query: &str) -> Query {
        let mut out = Query::default();
        let pattern = query.trim();
        if !pattern.is_empty() {
            out.push_regex(pattern, false);
        }
        out
    }

    fn push_regex(&mut self, pattern: &str, without: bool) {
        match regex::bytes::RegexBuilder::new(pattern).case_insensitive(true).size_limit(1 << 22).build() {
            Ok(r) => self.terms.push(Term::Regex(r, without)),
            Err(e) => {
                self.error = Some(short_error(&e));
                self.terms.push(Term::Never);
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.needle.is_empty() && self.typed.is_empty() && self.terms.is_empty()
    }

    /// Everything but the words: what the name, kind, size and date must be.
    fn passes(&self, lower_name: &[u8], is_dir: bool, size: u64, mtime: u32) -> bool {
        self.terms.iter().all(|term| match term {
            Term::Without(text) => memmem::find(lower_name, text.as_bytes()).is_none(),
            Term::Phrase(text) => memmem::find(lower_name, text.as_bytes()).is_some(),
            Term::Glob(pattern, without) => glob_match(pattern, lower_name) != *without,
            Term::Kind(kind) => kind.holds(lower_name, is_dir),
            Term::SizeMin(n) => !is_dir && size >= *n,
            Term::SizeMax(n) => !is_dir && size <= *n,
            Term::After(t) => mtime >= *t,
            Term::Before(t) => mtime < *t,
            Term::Regex(r, without) => r.is_match(lower_name) != *without,
            Term::Never => false,
        })
    }
}

// ---- Reading folders ----

#[derive(Default)]
pub struct Listing {
    name: String,
    mtime: u32,
    files: Vec<(Box<str>, u32, u64)>,
    dirs: Vec<Listing>,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct IndexParams {
    pub roots: Vec<String>,
    /// Folder names to skip wherever they are.
    pub exclude: Vec<String>,
    /// Exact folders to skip.
    pub exclude_paths: Vec<String>,
    /// File types to skip, with the dot.
    pub exclude_exts: Vec<String>,
    /// Regular expressions: an item whose whole path matches is left out, and a folder with all it holds.
    pub patterns: Vec<String>,
    /// Attributes that leave an item out, see platform::HIDDEN and platform::SYSTEM.
    pub skip_attrs: u32,
}

impl IndexParams {
    /// What's left out, as one number: when it changes, every place is read again. The places themselves aren't in
    /// it: one added or taken away doesn't make the others need reading.
    fn rules_fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.exclude.hash(&mut h);
        self.exclude_paths.hash(&mut h);
        self.exclude_exts.hash(&mut h);
        self.patterns.hash(&mut h);
        self.skip_attrs.hash(&mut h);
        h.finish()
    }

    fn rules(&self) -> Rules {
        Rules {
            names: self.exclude.iter().map(|e| matching::lower(e)).collect(),
            paths: self.exclude_paths.iter().map(|p| matching::lower(p.trim_end_matches('\\'))).collect(),
            exts: self.exclude_exts.iter().map(|e| matching::lower(e)).collect(),
            patterns: self.patterns.iter().filter_map(|p| pattern(p).ok()).collect(),
            skip_attrs: self.skip_attrs,
        }
    }
}

/// A pattern as search uses it: matched against the whole path, whatever the case.
pub fn pattern(text: &str) -> Result<regex::Regex, regex::Error> {
    regex::RegexBuilder::new(text).case_insensitive(true).build()
}

/// What to leave out while reading folders.
#[derive(Clone, Default)]
struct Rules {
    names: Vec<String>,
    paths: Vec<String>,
    exts: Vec<String>,
    patterns: Vec<regex::Regex>,
    skip_attrs: u32,
}

impl Rules {
    fn skips_folder(&self, parent: &str, name: &str) -> bool {
        self.names.iter().any(|e| e.eq_ignore_ascii_case(name))
            || (!self.paths.is_empty() && self.paths.contains(&matching::lower(&join(parent, name))))
            || self.matches(parent, name)
    }

    fn skips_file(&self, parent: &str, name: &str) -> bool {
        let by_type = !self.exts.is_empty() && {
            let lowered = matching::lower(name);
            self.exts.iter().any(|ext| lowered.ends_with(ext.as_str()))
        };
        by_type || self.matches(parent, name)
    }

    fn matches(&self, parent: &str, name: &str) -> bool {
        !self.patterns.is_empty() && {
            let path = join(parent, name);
            self.patterns.iter().any(|p| p.is_match(&path))
        }
    }

    /// For a change Windows reports: `rel` is the path below the searched place.
    fn skips_change(&self, full: &str, rel: &str) -> bool {
        let by_type = !self.exts.is_empty() && {
            let lowered = matching::lower(rel);
            self.exts.iter().any(|ext| lowered.ends_with(ext.as_str()))
        };
        if path_is_excluded(rel, &self.names) || by_type {
            return true;
        }
        // A pattern that matches the item or any folder it's in below the searched place.
        let top = full.len() - rel.len();
        let mut end = full.len();
        while !self.patterns.is_empty() && end > top {
            if self.patterns.iter().any(|p| p.is_match(&full[..end])) {
                return true;
            }
            end = full[..end].rfind('\\').unwrap_or(0);
        }
        let lowered = matching::lower(full);
        self.paths.iter().any(|p| lowered == *p || lowered.strip_prefix(p.as_str()).is_some_and(|rest| rest.starts_with('\\')))
    }
}

fn walk(path: &str, name: String, mtime: u32, rules: &Rules, counter: &AtomicUsize, gate: &Gate) -> Listing {
    let mut listing = Listing { name, mtime, ..Default::default() };
    let mut subdirs = Vec::new();
    gate.before_listing();
    platform::list_dir(path, rules.skip_attrs, |entry_name, is_dir, is_link, entry_mtime, entry_size| {
        if is_dir {
            if rules.skips_folder(path, entry_name) {
                return;
            }
            if is_link {
                // Links can loop back up the tree: list them but never walk through them.
                listing.files.push((entry_name.into(), entry_mtime, entry_size));
            } else {
                subdirs.push((entry_name.to_string(), entry_mtime));
            }
        } else if !rules.skips_file(path, entry_name) {
            listing.files.push((entry_name.into(), entry_mtime, entry_size));
        }
    });
    counter.fetch_add(listing.files.len() + subdirs.len(), Ordering::Relaxed);
    listing.dirs = subdirs
        .into_par_iter()
        .map(|(dir, dir_mtime)| {
            let child = join(path, &dir);
            walk(&child, dir, dir_mtime, rules, counter, gate)
        })
        .collect();
    listing
}

fn join(base: &str, name: &str) -> String {
    let mut out = String::with_capacity(base.len() + name.len() + 1);
    out.push_str(base);
    if !out.ends_with('\\') {
        out.push('\\');
    }
    out.push_str(name);
    out
}


// ---- Keeping it fresh ----

#[derive(Debug, Clone)]
pub enum Change {
    Added(String),
    Removed(String),
    Modified(String),
}

impl Index {
    fn apply(&mut self, change: &Change, params: &IndexParams, rules: &Rules) {
        match change {
            Change::Removed(path) => {
                if let Some(idx) = self.find_path(path) {
                    self.entries[idx].flags |= FLAG_DEAD;
                }
            }
            Change::Modified(path) => {
                if let Some(idx) = self.find_path(path) {
                    if let Some((_, mtime)) = platform::stat(path) {
                        self.entries[idx].mtime = mtime;
                        self.entries[idx].size = platform::file_size(path).unwrap_or(0);
                    }
                }
            }
            Change::Added(path) => {
                if self.find_path(path).is_some() {
                    return;
                }
                let Some((parent_path, name)) = path.rsplit_once('\\') else { return };
                let Some(parent) = self.find_path(parent_path) else { return };
                let Some((is_dir, mtime)) = platform::stat(path) else { return };
                if platform::has_attributes(path, params.skip_attrs) {
                    return;
                }
                let depth = self.entries[parent].depth.saturating_add(1);
                if is_dir {
                    let counter = AtomicUsize::new(0);
                    let listing = walk(path, name.to_string(), mtime, rules, &counter, &Gate::none());
                    self.append_listing(parent as u32, depth, &listing);
                } else {
                    self.push_sized(name, parent as u32, depth, mtime, platform::file_size(path).unwrap_or(0), 0);
                }
            }
        }
    }
}

fn path_is_excluded(path: &str, excluded: &[String]) -> bool {
    path.split('\\').any(|part| excluded.iter().any(|e| e.eq_ignore_ascii_case(part)))
}


#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Index {
        let mut index = Index::default();
        let root = index.push("C:\\Users\\me", NO_PARENT, 0, 0, FLAG_DIR | FLAG_ROOT);
        let docs = index.push("Documents", root, 1, 1, FLAG_DIR);
        let taxes = index.push("Taxes 2025", docs, 2, 1, FLAG_DIR);
        index.push("Report.pdf", taxes, 3, 1, 0);
        index.push("Quarterly Report Final.docx", docs, 2, 1, 0);
        index.push("Größe.txt", docs, 2, 1, 0);
        index.push("holiday.jpg", root, 1, 1, 0);
        index
    }

    #[test]
    fn exact_name_beats_longer_name_and_path_is_rebuilt() {
        let hits = sample().search("report", 10, &|| false);
        assert_eq!(hits[0].name, "Report.pdf");
        assert_eq!(hits[0].path, "C:\\Users\\me\\Documents\\Taxes 2025\\Report.pdf");
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn extra_words_can_name_a_parent_folder() {
        let hits = sample().search("report taxes", 10, &|| false);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "Report.pdf");
    }

    #[test]
    fn extension_filter_works_alone_and_with_words() {
        assert_eq!(sample().search("ext:pdf", 10, &|| false).len(), 1);
        assert_eq!(sample().search("ext:docx report", 10, &|| false)[0].name, "Quarterly Report Final.docx");
    }

    #[test]
    fn non_english_names_are_found_in_any_case() {
        assert_eq!(sample().search("GRÖSSE", 10, &|| false).len(), 0, "ß and SS are different letters here");
        assert_eq!(sample().search("größe", 10, &|| false)[0].name, "Größe.txt");
        assert_eq!(sample().search("GRÖßE", 10, &|| false)[0].name, "Größe.txt");
    }

    #[test]
    fn gaps_are_only_a_fallback() {
        assert_eq!(sample().search("hldy", 10, &|| false)[0].name, "holiday.jpg");
    }

    #[test]
    fn scattered_letters_stay_out_when_a_real_match_exists() {
        let mut index = sample();
        index.push("keynote.json", 0, 1, 1, 0);
        index.push("known_marketplaces.json", 0, 1, 1, 0);
        let hits = index.search("note", 10, &|| false);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "keynote.json");
        assert_eq!(index.search("nwnmrk", 10, &|| false)[0].name, "known_marketplaces.json");
    }

    #[test]
    fn a_shares_part_is_kept_when_the_local_drives_are_read_again() {
        let mut old = sample();
        let share = old.push("\\\\nas\\photos", NO_PARENT, 0, 0, FLAG_DIR | FLAG_ROOT);
        let trip = old.push("Trip", share, 1, 1, FLAG_DIR);
        old.push("beach.jpg", trip, 2, 1, 0);
        assert_eq!(old.root_names(), ["C:\\Users\\me", "\\\\nas\\photos"]);

        // A fresh walk of the local place only, then the share's part from before is added back.
        let mut fresh = Index::default();
        let root = fresh.push("C:\\Users\\me", NO_PARENT, 0, 0, FLAG_DIR | FLAG_ROOT);
        fresh.push("new.txt", root, 1, 1, 0);
        let mut merged = fresh.copy_roots(&|_| true);
        merged.append_roots(&old, &|root| root.starts_with("\\\\"));
        assert_eq!(merged.root_names(), ["C:\\Users\\me", "\\\\nas\\photos"]);
        assert_eq!(merged.search("beach", 10, &|| false)[0].path, "\\\\nas\\photos\\Trip\\beach.jpg");
        assert_eq!(merged.search("new.txt", 10, &|| false).len(), 1);
        assert!(merged.search("holiday", 10, &|| false).is_empty(), "the old local part is replaced, not kept twice");
    }

    #[test]
    fn a_match_cannot_span_two_names() {
        assert!(sample().search("pdfquarterly", 10, &|| false).is_empty());
    }

    #[test]
    fn removed_folder_hides_everything_inside_it() {
        let mut index = sample();
        let params = IndexParams::default();
        index.apply(&Change::Removed("C:\\Users\\me\\Documents\\Taxes 2025".into()), &params, &Rules::default());
        let hits = index.search("report", 10, &|| false);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "Quarterly Report Final.docx");
    }

    #[test]
    fn a_big_index_stays_fast() {
        let mut index = Index::default();
        let root = index.push("C:\\data", NO_PARENT, 0, 0, FLAG_DIR | FLAG_ROOT);
        for i in 0..1_000_000u32 {
            index.push(&format!("document-{i:07}-notes.txt"), root, 1, 0, 0);
        }
        index.push("needle-in-a-haystack.pdf", root, 1, 0, 0);
        let started = Instant::now();
        let hits = index.search("haystack", 10, &|| false);
        let rare = started.elapsed();
        assert_eq!(hits[0].name, "needle-in-a-haystack.pdf");
        let started = Instant::now();
        let hits = index.search("notes", 30, &|| false);
        let common = started.elapsed();
        assert_eq!(hits.len(), 30);
        eprintln!("1,000,001 names: rare word {rare:?}, word in every name {common:?}");
    }

    fn filtered(index: &Index, query: &str, exts: &[&str], folders: bool) -> Answer {
        let filter = FileFilter { exts: exts.iter().map(|e| e.to_string()).collect(), folders, ..Default::default() };
        index.search_filtered(query, &filter, 10, &|| false)
    }

    #[test]
    fn typing_a_file_type_filters_with_or_without_words() {
        assert_eq!(sample().search(".pdf", 10, &|| false)[0].name, "Report.pdf");
        assert_eq!(sample().search("*.pdf", 10, &|| false).len(), 1);
        assert_eq!(sample().search("report .docx", 10, &|| false)[0].name, "Quarterly Report Final.docx");
        assert_eq!(sample().search(".pdf .jpg", 10, &|| false).len(), 2);
        assert_eq!(typed_extension("ext:PDF"), Some(".pdf".to_string()));
        assert_eq!(typed_extension("report"), None);
        assert_eq!(typed_extension("."), None);
    }

    #[test]
    fn the_counts_stay_put_while_a_filter_is_on() {
        let answer = filtered(&sample(), "report", &[".pdf"], false);
        assert_eq!(answer.hits.len(), 1);
        assert_eq!(answer.hits[0].name, "Report.pdf");
        assert_eq!(answer.facets.total, 2);
        assert!(answer.facets.exts.contains(&(".docx".to_string(), 1)), "the other type is still offered");
    }

    #[test]
    fn folders_can_be_picked_out() {
        let answer = filtered(&sample(), "taxes", &[], true);
        assert_eq!(answer.hits[0].name, "Taxes 2025");
        assert_eq!(answer.facets.folders, 1);
        assert!(filtered(&sample(), "report", &[], true).hits.is_empty());
    }

    #[test]
    fn a_filter_that_empties_the_list_does_not_bring_back_scattered_letters() {
        assert!(filtered(&sample(), "report", &[".png"], false).hits.is_empty());
    }

    #[test]
    fn only_real_file_types_are_counted() {
        let mut tally = Tally::default();
        for name in ["a.pdf", "b.pdf", "v1.2", ".gitignore", "report.final draft", "noext", "c.tar.gz"] {
            tally.add(name.as_bytes(), false);
        }
        let facets = tally.finish();
        assert_eq!(facets.total, 7);
        assert_eq!(facets.exts, vec![(".pdf".to_string(), 2), (".gz".to_string(), 1)]);
    }

    #[test]
    fn skip_rules_cover_names_paths_and_file_types() {
        let rules = IndexParams {
            exclude: vec!["AppData".into()],
            exclude_paths: vec!["C:\\Users\\me\\Secret".into()],
            exclude_exts: vec![".tmp".into()],
            ..Default::default()
        }
        .rules();
        assert!(rules.skips_folder("C:\\Users\\me", "appdata"));
        assert!(rules.skips_folder("C:\\Users\\me", "SECRET"));
        assert!(!rules.skips_folder("C:\\Users\\me", "Secrets"));
        assert!(!rules.skips_folder("C:\\Users\\other", "Secret"));
        assert!(rules.skips_file("C:\\x", "Cache.TMP"));
        assert!(!rules.skips_file("C:\\x", "notes.txt"));
        assert!(rules.skips_change("C:\\Users\\me\\Secret\\a.txt", "Secret\\a.txt"));
        assert!(!rules.skips_change("C:\\Users\\me\\Secrets\\a.txt", "Secrets\\a.txt"));
    }

    #[test]
    fn patterns_match_the_whole_path_and_leave_out_what_a_folder_holds() {
        let rules = IndexParams { patterns: vec![r"\\payroll\\".into(), r"\.(key|pem)$".into(), "(unclosed".into()], ..Default::default() }.rules();
        assert_eq!(rules.patterns.len(), 2, "a pattern that isn't valid is left out");
        assert!(rules.skips_file("C:\\Work", "server.KEY"), "whatever the case");
        assert!(!rules.skips_file("C:\\Work", "keys.txt"));
        assert!(!rules.skips_folder("C:\\Work", "Payroll"), "the folder's own path doesn't end in a backslash");
        assert!(rules.skips_change("C:\\Work\\Payroll\\2026\\june.xlsx", "Payroll\\2026\\june.xlsx"), "what it holds is left out");
        assert!(!rules.skips_change("C:\\Work\\Pay\\june.xlsx", "Pay\\june.xlsx"));
        let folder = IndexParams { patterns: vec![r"\\payroll$".into()], ..Default::default() }.rules();
        assert!(folder.skips_folder("C:\\Work", "Payroll"));
        assert!(folder.skips_change("C:\\Work\\Payroll\\june.xlsx", "Payroll\\june.xlsx"));
    }

    #[test]
    fn counts_are_kept_per_searched_place() {
        let mut index = sample();
        let second = index.push("D:\\work", NO_PARENT, 0, 0, FLAG_DIR | FLAG_ROOT);
        index.push("plan.txt", second, 1, 1, 0);
        let counts = index.counts_by_root();
        assert_eq!(counts[0], ("C:\\Users\\me".to_string(), 6));
        assert_eq!(counts[1], ("D:\\work".to_string(), 1));
    }

    #[test]
    fn exclusion_matches_whole_folder_names_only() {
        let excluded = vec!["target".to_string()];
        assert!(path_is_excluded("C:\\code\\target\\debug", &excluded));
        assert!(!path_is_excluded("C:\\code\\targets\\debug", &excluded));
    }
}

#[cfg(test)]
mod query_tests {
    use super::*;

    /// A few files with sizes and dates, all in one folder.
    fn files() -> Index {
        let mut index = Index::default();
        let root = index.push(r"C:\work", NO_PARENT, 0, 0, FLAG_DIR | FLAG_ROOT);
        let day = |y, m, d| platform::local_midnight(y, m, d).unwrap() as u32 + 3600;
        index.push_sized("Report final.docx", root, 1, day(2025, 3, 14), 40_000, 0);
        index.push_sized("report draft.docx", root, 1, day(2026, 1, 2), 9_000, 0);
        index.push_sized("img_01.png", root, 1, day(2026, 1, 2), 2_500_000, 0);
        index.push_sized("img_123.png", root, 1, day(2024, 6, 1), 300, 0);
        index.push_sized("movie.mkv", root, 1, day(2026, 1, 3), 3 << 30, 0);
        index.push_sized("Photos", root, 1, day(2026, 1, 3), 0, FLAG_DIR);
        index
    }

    fn names(query: &str) -> Vec<String> {
        let mut found: Vec<String> = files().search_filtered(query, &FileFilter::default(), 50, &|| false).hits.into_iter().map(|h| h.name).collect();
        found.sort();
        found
    }

    #[test]
    fn words_quotes_wildcards_and_without() {
        assert_eq!(names("report"), ["Report final.docx", "report draft.docx"]);
        assert_eq!(names("report -draft"), ["Report final.docx"]);
        assert_eq!(names("\"report final\""), ["Report final.docx"]);
        assert_eq!(names("\"final report\""), Vec::<String>::new(), "a phrase keeps its order");
        assert_eq!(names("img_??.png"), ["img_01.png"]);
        assert_eq!(names("*.png"), ["img_01.png", "img_123.png"], "a plain *.type is a file type");
        assert_eq!(names("img*"), ["img_01.png", "img_123.png"]);
        assert_eq!(names("-*.png report"), ["Report final.docx", "report draft.docx"]);
    }

    #[test]
    fn types_kinds_sizes_and_dates() {
        assert_eq!(names("ext:png,mkv"), ["img_01.png", "img_123.png", "movie.mkv"]);
        assert_eq!(names("kind:picture"), ["img_01.png", "img_123.png"]);
        assert_eq!(names("kind:folder"), ["Photos"]);
        assert_eq!(names("size:>1mb"), ["img_01.png", "movie.mkv"]);
        assert_eq!(names("size:<1k"), ["img_123.png"]);
        assert_eq!(names("size:1k..100k"), ["Report final.docx", "report draft.docx"]);
        assert_eq!(names("modified:2025"), ["Report final.docx"]);
        assert_eq!(names("date:2026-01-02"), ["img_01.png", "report draft.docx"]);
        assert_eq!(names("date:02.01.2026"), ["img_01.png", "report draft.docx"], "as German writes it");
        assert_eq!(names("modified:<2025-01-01"), ["img_123.png"]);
        assert_eq!(names("modified:>2026-01-01 kind:video"), ["movie.mkv"]);
    }

    #[test]
    fn regular_expressions() {
        assert_eq!(names(r"re:^img_\d{2}\."), ["img_01.png"]);
        assert_eq!(names(r"/^IMG_\d+/"), ["img_01.png", "img_123.png"], "whatever the case");
        assert_eq!(names(r"-re:draft report"), ["Report final.docx"]);
        let answer = files().search_filtered("re:([unclosed", &FileFilter::default(), 50, &|| false);
        assert!(answer.hits.is_empty() && answer.error.is_some(), "an invalid pattern finds nothing and says why");
        let whole = FileFilter { regex: true, ..Default::default() };
        let found = files().search_filtered(r"^report (final|draft)\.docx$", &whole, 50, &|| false).hits;
        assert_eq!(found.len(), 2, "in regex mode the whole search is one pattern, spaces and all");
    }

    #[test]
    fn only_file_syntax_counts_as_file_syntax() {
        for query in ["ext:pdf", "kind:image", "size:>1mb", "modified:today", "re:x", "/x/", "*.tmp", "img_??.png", "-*.log"] {
            assert!(uses_file_syntax(query), "{query}");
        }
        for query in ["dark mode", "volume 40", r"c:\users", "notes: plan", "what is my ip?"] {
            assert!(!uses_file_syntax(query), "{query}");
        }
    }
}
