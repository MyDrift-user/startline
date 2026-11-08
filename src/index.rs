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
    pub score: i32,
}

impl Index {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    fn push(&mut self, name: &str, parent: u32, depth: u8, mtime: u32, flags: u8) -> u32 {
        let name_bytes = &name.as_bytes()[..name.len().min(u16::MAX as usize - 1)];
        let lowered = matching::lower(name);
        let lower_bytes = &lowered.as_bytes()[..lowered.len().min(u16::MAX as usize - 1)];
        let entry = Entry {
            lower_off: self.lower.len() as u32,
            orig_off: self.orig.len() as u32,
            parent,
            mtime,
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
        for (name, mtime) in &listing.files {
            self.push(name, me, depth, *mtime, 0);
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
        let parsed = Query::parse(query);
        if self.entries.is_empty() || (parsed.needle.is_empty() && parsed.typed.is_empty()) {
            return Answer::default();
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
                    score,
                }
            })
            .collect();
        Answer { hits, facets }
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

    fn load(fingerprint: u64) -> Option<Index> {
        let bytes = std::fs::read(cache_path()).ok()?;
        let word = |i: usize| -> Option<u64> { Some(u64::from_le_bytes(bytes.get(i * 8..i * 8 + 8)?.try_into().ok()?)) };
        if word(0)? != CACHE_MAGIC || word(1)? != fingerprint {
            return None;
        }
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
        sound.then_some(Index { entries, lower, orig })
    }
}

const CACHE_MAGIC: u64 = 0x5354_4c49_4e45_3031;

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
}

impl FileFilter {
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

struct Query {
    /// The longest word: the rarest, so it drives the scan.
    needle: String,
    others: Vec<String>,
    typed: Vec<String>,
}

impl Query {
    fn parse(query: &str) -> Query {
        let mut words = matching::tokens(query);
        let mut typed = Vec::new();
        words.retain(|word| match typed_extension(word) {
            Some(ext) => {
                typed.push(ext);
                false
            }
            None => true,
        });
        let longest = words.iter().enumerate().max_by_key(|(_, w)| w.len()).map(|(i, _)| i);
        let needle = match longest {
            Some(i) => words.remove(i),
            None => String::new(),
        };
        Query { needle, others: words, typed }
    }
}

// ---- Reading folders ----

#[derive(Default)]
pub struct Listing {
    name: String,
    mtime: u32,
    files: Vec<(Box<str>, u32)>,
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
    fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.roots.hash(&mut h);
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

fn walk(path: &str, name: String, mtime: u32, rules: &Rules, counter: &AtomicUsize) -> Listing {
    let mut listing = Listing { name, mtime, ..Default::default() };
    let mut subdirs = Vec::new();
    platform::list_dir(path, rules.skip_attrs, |entry_name, is_dir, is_link, entry_mtime| {
        if is_dir {
            if rules.skips_folder(path, entry_name) {
                return;
            }
            if is_link {
                // Links can loop back up the tree: list them but never walk through them.
                listing.files.push((entry_name.into(), entry_mtime));
            } else {
                subdirs.push((entry_name.to_string(), entry_mtime));
            }
        } else if !rules.skips_file(path, entry_name) {
            listing.files.push((entry_name.into(), entry_mtime));
        }
    });
    counter.fetch_add(listing.files.len() + subdirs.len(), Ordering::Relaxed);
    listing.dirs = subdirs
        .into_par_iter()
        .map(|(dir, dir_mtime)| {
            let child = join(path, &dir);
            walk(&child, dir, dir_mtime, rules, counter)
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

/// Reads all folders. `store` receives finished pieces, so a first-time index can
/// answer searches while it is still growing.
fn walk_all(params: &IndexParams, counter: &AtomicUsize, store: &mut dyn FnMut(&mut dyn FnMut(&mut Index))) {
    let rules = params.rules();
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 8);
    let Ok(pool) = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .start_handler(|_| platform::lower_thread_priority())
        .build()
    else {
        return;
    };
    for root in &params.roots {
        if !std::path::Path::new(root).is_dir() {
            continue;
        }
        let mut root_entry = NO_PARENT;
        store(&mut |index| root_entry = index.push(root, NO_PARENT, 0, 0, FLAG_DIR | FLAG_ROOT));
        // Read the top level first, then finish one top-level folder at a time.
        let mut top = Listing::default();
        let mut subdirs = Vec::new();
        platform::list_dir(root, params.skip_attrs, |name, is_dir, is_link, mtime| {
            if is_dir {
                if rules.skips_folder(root, name) {
                    return;
                }
                if is_link {
                    top.files.push((name.into(), mtime));
                } else {
                    subdirs.push((name.to_string(), mtime));
                }
            } else if !rules.skips_file(root, name) {
                top.files.push((name.into(), mtime));
            }
        });
        counter.fetch_add(top.files.len(), Ordering::Relaxed);
        store(&mut |index| index.append_children(root_entry, 1, &top));
        for (dir, mtime) in subdirs {
            let child = join(root, &dir);
            let listing = pool.install(|| walk(&child, dir, mtime, &rules, counter));
            store(&mut |index| index.append_listing(root_entry, 1, &listing));
        }
    }
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
                    let listing = walk(path, name.to_string(), mtime, rules, &counter);
                    self.append_listing(parent as u32, depth, &listing);
                } else {
                    self.push(name, parent as u32, depth, mtime, 0);
                }
            }
        }
    }
}

fn path_is_excluded(path: &str, excluded: &[String]) -> bool {
    path.split('\\').any(|part| excluded.iter().any(|e| e.eq_ignore_ascii_case(part)))
}

enum Cmd {
    Reindex(IndexParams),
    Change(Change),
    Overflow,
    WalkDone(Index, IndexParams),
}

pub struct IndexStatus {
    pub walking: AtomicBool,
    pub walked: AtomicUsize,
    /// How long the last full read of all folders took, in milliseconds.
    pub last_walk_ms: AtomicU64,
}

pub struct IndexManager {
    pub index: Arc<RwLock<Index>>,
    pub status: Arc<IndexStatus>,
    tx: Sender<Cmd>,
}

impl IndexManager {
    /// Loads the saved index right away, then refreshes it in the background.
    pub fn start(params: IndexParams) -> IndexManager {
        let cached = Index::load(params.fingerprint()).unwrap_or_default();
        let index = Arc::new(RwLock::new(cached));
        let status = Arc::new(IndexStatus {
            walking: AtomicBool::new(false),
            walked: AtomicUsize::new(0),
            last_walk_ms: AtomicU64::new(0),
        });
        let (tx, rx) = mpsc::channel::<Cmd>();
        let manager = IndexManager { index: index.clone(), status: status.clone(), tx: tx.clone() };
        let _ = tx.send(Cmd::Reindex(params));

        std::thread::Builder::new()
            .name("index".into())
            .spawn(move || {
                let mut active: Option<IndexParams> = None;
                let mut rules = Rules::default();
                let mut watchers: Vec<platform::DirWatcher> = Vec::new();
                let mut pending: Vec<Change> = Vec::new();
                let mut walk_running = false;
                let mut queued: Option<IndexParams> = None;
                let mut refresh_at: Option<Instant> = None;
                let mut dirty = false;
                let mut last_save = Instant::now();

                let begin_walk = |p: IndexParams, index: &Arc<RwLock<Index>>, status: &Arc<IndexStatus>, tx: &Sender<Cmd>| {
                    let (index, status, tx) = (index.clone(), status.clone(), tx.clone());
                    status.walking.store(true, Ordering::Relaxed);
                    status.walked.store(0, Ordering::Relaxed);
                    let _ = std::thread::Builder::new().name("index-walk".into()).spawn(move || {
                        let started = Instant::now();
                        let first_time = index.read().map(|i| i.len() == 0).unwrap_or(false);
                        let mut private = Index::default();
                        walk_all(&p, &status.walked, &mut |edit: &mut dyn FnMut(&mut Index)| {
                            if first_time {
                                if let Ok(mut live) = index.write() {
                                    edit(&mut live);
                                }
                            } else {
                                edit(&mut private);
                            }
                        });
                        status.last_walk_ms.store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
                        let finished = if first_time {
                            index.write().map(|mut live| std::mem::take(&mut *live)).unwrap_or_default()
                        } else {
                            private
                        };
                        let _ = tx.send(Cmd::WalkDone(finished, p));
                    });
                };

                loop {
                    let wait = refresh_at
                        .map(|at| at.saturating_duration_since(Instant::now()))
                        .unwrap_or(Duration::from_secs(600));
                    match rx.recv_timeout(wait) {
                        Ok(Cmd::Reindex(p)) => {
                            rules = p.rules();
                            watchers.clear();
                            for root in &p.roots {
                                let tx = tx.clone();
                                let rules = rules.clone();
                                let root_path = root.clone();
                                if let Some(w) = platform::DirWatcher::start(root, move |event| {
                                    let message = match event {
                                        platform::DirEvent::Overflow => Cmd::Overflow,
                                        platform::DirEvent::Added(rel) => {
                                            let path = join(&root_path, &rel);
                                            if rules.skips_change(&path, &rel) { return; }
                                            Cmd::Change(Change::Added(path))
                                        }
                                        platform::DirEvent::Removed(rel) => {
                                            let path = join(&root_path, &rel);
                                            if rules.skips_change(&path, &rel) { return; }
                                            Cmd::Change(Change::Removed(path))
                                        }
                                        platform::DirEvent::Modified(rel) => {
                                            let path = join(&root_path, &rel);
                                            if rules.skips_change(&path, &rel) { return; }
                                            Cmd::Change(Change::Modified(path))
                                        }
                                    };
                                    let _ = tx.send(message);
                                }) {
                                    watchers.push(w);
                                }
                            }
                            active = Some(p.clone());
                            if walk_running {
                                queued = Some(p);
                            } else {
                                walk_running = true;
                                pending.clear();
                                begin_walk(p, &index, &status, &tx);
                            }
                        }
                        Ok(Cmd::Change(change)) => {
                            let Some(p) = &active else { continue };
                            if let Ok(mut live) = index.write() {
                                live.apply(&change, p, &rules);
                            }
                            dirty = true;
                            if walk_running {
                                pending.push(change);
                                // A huge burst (an unpacked archive, say): the walk may have seen some of it,
                                // so read everything again once rather than replaying thousands of events.
                                if pending.len() > 20_000 {
                                    pending.clear();
                                    queued = active.clone();
                                }
                            }
                        }
                        Ok(Cmd::Overflow) => {
                            // Too much changed at once for Windows to list. Look again shortly.
                            refresh_at.get_or_insert_with(|| Instant::now() + Duration::from_secs(60));
                        }
                        Ok(Cmd::WalkDone(mut finished, p)) => {
                            walk_running = false;
                            status.walking.store(false, Ordering::Relaxed);
                            if active.as_ref() == Some(&p) {
                                for change in pending.drain(..) {
                                    finished.apply(&change, &p, &rules);
                                }
                                let _ = finished.save(p.fingerprint());
                                if let Ok(mut live) = index.write() {
                                    *live = finished;
                                }
                                dirty = false;
                                last_save = Instant::now();
                            }
                            if let Some(next) = queued.take() {
                                walk_running = true;
                                pending.clear();
                                begin_walk(next, &index, &status, &tx);
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            if refresh_at.is_some_and(|at| at <= Instant::now()) {
                                refresh_at = None;
                                if let (Some(p), false) = (active.clone(), walk_running) {
                                    walk_running = true;
                                    pending.clear();
                                    begin_walk(p, &index, &status, &tx);
                                }
                            }
                            if dirty && !walk_running && last_save.elapsed() > Duration::from_secs(300) {
                                if let (Some(p), Ok(live)) = (&active, index.read()) {
                                    let _ = live.save(p.fingerprint());
                                }
                                dirty = false;
                                last_save = Instant::now();
                            }
                        }
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .expect("index thread");
        manager
    }

    pub fn reindex(&self, params: IndexParams) {
        let _ = self.tx.send(Cmd::Reindex(params));
    }
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
        let filter = FileFilter { exts: exts.iter().map(|e| e.to_string()).collect(), folders };
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
