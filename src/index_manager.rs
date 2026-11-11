//! Keeping the index fresh, gently. The saved index answers searches from the first moment; a place is read again
//! from disk only when it has to be, in Windows's background mode, with as few threads as its disk takes, never while
//! a full-screen app is in front, and only after the first ten minutes on battery. Network shares get no watch on them and are read at most
//! once a day, only while the PC is idle, at a slow and steady pace.

use super::*;
use crate::platform::PlaceKind;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

const DAY: u64 = 86_400;
/// How long the PC has to be left alone before a network share is read.
const IDLE_BEFORE_NETWORK: u64 = 5 * 60;
/// Folder listings a second on a network share.
const NETWORK_LISTINGS_PER_SECOND: u32 = 20;

/// What to do about one searched place.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Plan {
    /// Read it now.
    Walk,
    /// Read it next time the PC is idle.
    WalkWhenIdle,
    /// The saved index is good: keep it, and the watch keeps it fresh.
    Keep,
}

/// Whether a place needs a full read. A watched place (a local or USB drive) does on the first run, when what's left
/// out changed, after a watch overflowed, or when its saved part is more than a day old; otherwise the saved index and
/// the watch are trusted. A network share is never watched, so it's read when it isn't in the index yet, or once its
/// part is a day old, and only while the PC is idle.
pub fn plan(kind: PlaceKind, in_index: bool, last_walk: Option<u64>, rules_changed: bool, overflowed: bool, now: u64) -> Plan {
    let stale = last_walk.is_none_or(|at| now.saturating_sub(at) >= DAY);
    if !kind.watched() {
        if !in_index || rules_changed || stale { Plan::WalkWhenIdle } else { Plan::Keep }
    } else if !in_index || rules_changed || overflowed || stale {
        Plan::Walk
    } else {
        Plan::Keep
    }
}

/// When each place was last read in full, kept next to the saved index.
#[derive(Default, Serialize, Deserialize, Debug, PartialEq)]
#[serde(default)]
pub struct WalkLog {
    /// What was left out when they were read: when it changes, every place is read again.
    pub rules: u64,
    /// Place, and when it was last read in full, in seconds since 1970.
    pub walked: HashMap<String, u64>,
    /// Places whose watch lost track of changes since they were last read.
    pub overflowed: Vec<String>,
}

impl WalkLog {
    fn path() -> PathBuf {
        crate::config::data_dir().join("index-state.json")
    }

    fn load() -> WalkLog {
        std::fs::read(Self::path()).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    fn save(&self) {
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            let tmp = Self::path().with_extension("json.tmp");
            if std::fs::write(&tmp, bytes).is_ok() {
                let _ = std::fs::rename(&tmp, Self::path());
            }
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// How long a walk waits after the PC goes on battery. A laptop that stays on battery for good still gets its index
/// read, in background mode as always.
const BATTERY_WAIT: Duration = Duration::from_secs(10 * 60);

/// Whether a walk should wait now: while a full-screen app is in front, and for the first minutes on battery.
pub fn waits(on_battery: bool, battery_for: Duration, full_screen: bool) -> bool {
    full_screen || (on_battery && battery_for < BATTERY_WAIT)
}

/// Waits before each folder listing while it's no time to read the disk, and keeps a slow place to its pace.
pub struct Gate {
    /// Wait while on battery or while a full-screen app is in front.
    pause: bool,
    /// At most this many listings a second.
    per_second: Option<u32>,
    last: Mutex<Option<Instant>>,
    /// The last look at battery and full screen, so they're asked about only every few seconds.
    checked: Mutex<Option<(Instant, bool)>>,
}

impl Gate {
    /// No waiting at all, for the few folders a watch reports.
    pub fn none() -> Gate {
        Gate { pause: false, per_second: None, last: Mutex::new(None), checked: Mutex::new(None) }
    }

    fn for_place(kind: PlaceKind) -> Gate {
        let per_second = (kind == PlaceKind::Network).then_some(NETWORK_LISTINGS_PER_SECOND);
        Gate { pause: true, per_second, last: Mutex::new(None), checked: Mutex::new(None) }
    }

    /// Whether it's a bad moment to read the disk: just gone on battery, or a full-screen app in front.
    fn busy(&self) -> bool {
        // Since when the PC has been on battery, for every place being read.
        static ON_BATTERY_SINCE: Mutex<Option<Instant>> = Mutex::new(None);
        let mut checked = self.checked.lock().unwrap();
        if let Some((at, busy)) = *checked {
            if at.elapsed() < Duration::from_secs(3) {
                return busy;
            }
        }
        let on_battery = platform::on_battery();
        let battery_for = {
            let mut since = ON_BATTERY_SINCE.lock().unwrap();
            match (on_battery, *since) {
                (false, _) => *since = None,
                (true, None) => *since = Some(Instant::now()),
                (true, Some(_)) => {}
            }
            since.map_or(Duration::ZERO, |at| at.elapsed())
        };
        let busy = waits(on_battery, battery_for, platform::full_screen_in_front());
        *checked = Some((Instant::now(), busy));
        busy
    }

    pub fn before_listing(&self) {
        if self.pause {
            while self.busy() {
                std::thread::sleep(Duration::from_secs(5));
            }
        }
        if let Some(per_second) = self.per_second {
            let gap = Duration::from_secs(1) / per_second.max(1);
            let mut last = self.last.lock().unwrap();
            if let Some(at) = *last {
                let since = at.elapsed();
                if since < gap {
                    std::thread::sleep(gap - since);
                }
            }
            *last = Some(Instant::now());
        }
    }
}

/// Reads these places, each with as many threads as its disk takes. `store` receives finished pieces, so a first-time
/// index can answer searches while it is still growing.
fn walk_places(places: &[(String, PlaceKind)], params: &IndexParams, counter: &AtomicUsize, store: &mut dyn FnMut(&mut dyn FnMut(&mut Index))) {
    let rules = params.rules();
    for (root, kind) in places {
        if !std::path::Path::new(root).is_dir() {
            continue;
        }
        let gate = Gate::for_place(*kind);
        let Ok(pool) = rayon::ThreadPoolBuilder::new().num_threads(kind.threads()).start_handler(|_| platform::background_mode()).build() else { continue };
        let mut root_entry = NO_PARENT;
        store(&mut |index| root_entry = index.push(root, NO_PARENT, 0, 0, FLAG_DIR | FLAG_ROOT));
        // Read the top level first, then finish one top-level folder at a time.
        let mut top = Listing::default();
        let mut subdirs = Vec::new();
        gate.before_listing();
        platform::list_dir(root, params.skip_attrs, |name, is_dir, is_link, mtime, size| {
            if is_dir {
                if rules.skips_folder(root, name) {
                    return;
                }
                if is_link {
                    top.files.push((name.into(), mtime, size));
                } else {
                    subdirs.push((name.to_string(), mtime));
                }
            } else if !rules.skips_file(root, name) {
                top.files.push((name.into(), mtime, size));
            }
        });
        counter.fetch_add(top.files.len(), Ordering::Relaxed);
        store(&mut |index| index.append_children(root_entry, 1, &top));
        for (dir, mtime) in subdirs {
            let child = join(root, &dir);
            let listing = pool.install(|| walk(&child, dir, mtime, &rules, counter, &gate));
            store(&mut |index| index.append_listing(root_entry, 1, &listing));
        }
    }
}

enum Cmd {
    Reindex(IndexParams),
    Change(Change),
    Overflow(String),
    WalkDone(Index, Vec<String>),
}

pub struct IndexStatus {
    pub walking: AtomicBool,
    pub walked: AtomicUsize,
    /// How long the last full read took, in milliseconds.
    pub last_walk_ms: AtomicU64,
}

pub struct IndexManager {
    pub index: Arc<RwLock<Index>>,
    pub status: Arc<IndexStatus>,
    tx: Sender<Cmd>,
}

/// Everything the index thread keeps track of.
struct Keeper {
    index: Arc<RwLock<Index>>,
    status: Arc<IndexStatus>,
    tx: Sender<Cmd>,
    active: Option<IndexParams>,
    rules: Rules,
    kinds: HashMap<String, PlaceKind>,
    log: WalkLog,
    watchers: Vec<platform::DirWatcher>,
    /// Changes reported while a walk runs, to replay on what it finds.
    pending: Vec<Change>,
    walking: Vec<String>,
    /// Places to read as soon as the running walk is done.
    due: Vec<String>,
    /// Network shares to read when the PC is next idle.
    due_idle: Vec<String>,
    dirty: bool,
    last_save: Instant,
}

impl Keeper {
    fn kind(&self, root: &str) -> PlaceKind {
        self.kinds.get(root).copied().unwrap_or(PlaceKind::Hdd)
    }

    fn reindex(&mut self, params: IndexParams) {
        let rules_fp = params.rules_fingerprint();
        let rules_changed = rules_fp != self.log.rules;
        if rules_changed {
            // What was read under other rules isn't trusted any more, anywhere.
            self.log.walked.clear();
            self.log.rules = rules_fp;
            self.log.save();
        }
        self.rules = params.rules();
        for root in &params.roots {
            if !self.kinds.contains_key(root) {
                self.kinds.insert(root.clone(), platform::place_kind(root));
            }
        }
        // Places no longer searched go, and a network share read under other rules goes until it's read again, so
        // nothing that's now left out is found meanwhile. A local place stays until its walk replaces it.
        let kinds = self.kinds.clone();
        let keep = |root: &str| params.roots.iter().any(|r| r == root) && !(rules_changed && kinds.get(root).is_some_and(|k| !k.watched()));
        if let Ok(mut live) = self.index.write() {
            if live.root_names().iter().any(|r| !keep(r)) {
                *live = live.copy_roots(&keep);
            }
        }
        self.watch(&params);
        let in_index: Vec<String> = self.index.read().map(|i| i.root_names()).unwrap_or_default();
        let now = now_secs();
        for root in &params.roots {
            let kind = self.kind(root);
            let last = self.log.walked.get(root).copied();
            let overflowed = self.log.overflowed.contains(root);
            match plan(kind, in_index.contains(root), last, rules_changed, overflowed, now) {
                Plan::Walk => push_once(&mut self.due, root),
                Plan::WalkWhenIdle => push_once(&mut self.due_idle, root),
                Plan::Keep => {}
            }
        }
        self.active = Some(params);
        self.start_due();
    }

    /// A watch on every place that takes one: local and USB drives, never network shares.
    fn watch(&mut self, params: &IndexParams) {
        self.watchers.clear();
        let watched: Vec<String> = params.roots.iter().filter(|r| self.kind(r).watched()).cloned().collect();
        for root in &watched {
            let (tx, rules, root_path) = (self.tx.clone(), self.rules.clone(), root.clone());
            let watcher = platform::DirWatcher::start(root, move |event| {
                let change = |rel: &str, make: fn(String) -> Change| {
                    let path = join(&root_path, rel);
                    (!rules.skips_change(&path, rel)).then(|| Cmd::Change(make(path)))
                };
                let message = match event {
                    platform::DirEvent::Overflow => Some(Cmd::Overflow(root_path.clone())),
                    platform::DirEvent::Added(rel) => change(&rel, Change::Added),
                    platform::DirEvent::Removed(rel) => change(&rel, Change::Removed),
                    platform::DirEvent::Modified(rel) => change(&rel, Change::Modified),
                };
                if let Some(message) = message {
                    let _ = tx.send(message);
                }
            });
            if let Some(watcher) = watcher {
                self.watchers.push(watcher);
            }
        }
    }

    /// Starts reading what's due, unless a walk is already running.
    fn start_due(&mut self) {
        if !self.walking.is_empty() || self.due.is_empty() {
            return;
        }
        let Some(params) = self.active.clone() else { return };
        let roots: Vec<String> = std::mem::take(&mut self.due).into_iter().filter(|r| params.roots.contains(r)).collect();
        self.begin(roots, params);
    }

    /// Network shares, once the PC has been left alone for a while.
    fn start_idle(&mut self) {
        if !self.walking.is_empty() || self.due_idle.is_empty() || platform::idle_seconds() < IDLE_BEFORE_NETWORK {
            return;
        }
        let Some(params) = self.active.clone() else { return };
        let roots: Vec<String> = std::mem::take(&mut self.due_idle).into_iter().filter(|r| params.roots.contains(r)).collect();
        self.begin(roots, params);
    }

    fn begin(&mut self, roots: Vec<String>, params: IndexParams) {
        if roots.is_empty() {
            return;
        }
        let places: Vec<(String, PlaceKind)> = roots.iter().map(|r| (r.clone(), self.kind(r))).collect();
        self.walking = roots.clone();
        self.pending.clear();
        let (index, status, tx) = (self.index.clone(), self.status.clone(), self.tx.clone());
        status.walking.store(true, Ordering::Relaxed);
        status.walked.store(0, Ordering::Relaxed);
        let _ = std::thread::Builder::new().name("index-walk".into()).spawn(move || {
            platform::background_mode();
            let started = Instant::now();
            let first_time = index.read().map(|i| i.len() == 0).unwrap_or(false);
            let mut private = Index::default();
            walk_places(&places, &params, &status.walked, &mut |edit: &mut dyn FnMut(&mut Index)| {
                if first_time {
                    if let Ok(mut live) = index.write() {
                        edit(&mut live);
                    }
                } else {
                    edit(&mut private);
                }
            });
            status.last_walk_ms.store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
            let finished = if first_time { index.write().map(|mut live| std::mem::take(&mut *live)).unwrap_or_default() } else { private };
            let _ = tx.send(Cmd::WalkDone(finished, roots));
        });
    }

    fn walk_done(&mut self, walked: Index, roots: Vec<String>) {
        self.walking.clear();
        self.status.walking.store(false, Ordering::Relaxed);
        let Some(params) = self.active.clone() else { return };
        // The places that weren't read keep their part of the index; the ones that were get what was just read.
        let mut merged = self.index.read().map(|live| live.copy_roots(&|r: &str| params.roots.iter().any(|p| p == r) && !roots.iter().any(|w| w == r))).unwrap_or_default();
        merged.append_roots(&walked, &|r: &str| params.roots.iter().any(|p| p == r));
        for change in self.pending.drain(..) {
            merged.apply(&change, &params, &self.rules);
        }
        let _ = merged.save(params.rules_fingerprint());
        if let Ok(mut live) = self.index.write() {
            *live = merged;
        }
        let now = now_secs();
        for root in &roots {
            self.log.walked.insert(root.clone(), now);
            self.log.overflowed.retain(|r| r != root);
        }
        self.log.save();
        self.dirty = false;
        self.last_save = Instant::now();
        self.start_due();
    }

    fn tick(&mut self) {
        // A network share whose day is up joins the queue for the next idle moment.
        if let Some(params) = self.active.clone() {
            let in_index: Vec<String> = self.index.read().map(|i| i.root_names()).unwrap_or_default();
            let now = now_secs();
            let shares: Vec<String> = params.roots.iter().filter(|r| !self.kind(r).watched()).cloned().collect();
            for root in &shares {
                if plan(PlaceKind::Network, in_index.contains(root), self.log.walked.get(root).copied(), false, false, now) == Plan::WalkWhenIdle && !self.walking.contains(root) {
                    push_once(&mut self.due_idle, root);
                }
            }
        }
        self.start_due();
        self.start_idle();
        if self.dirty && self.walking.is_empty() && self.last_save.elapsed() > Duration::from_secs(300) {
            if let (Some(params), Ok(live)) = (&self.active, self.index.read()) {
                let _ = live.save(params.rules_fingerprint());
            }
            self.dirty = false;
            self.last_save = Instant::now();
        }
    }
}

fn push_once(list: &mut Vec<String>, root: &str) {
    if !list.iter().any(|r| r == root) {
        list.push(root.to_string());
    }
}

impl IndexManager {
    /// Loads the saved index right away; what needs reading again is read in the background.
    pub fn start(params: IndexParams) -> IndexManager {
        let log = WalkLog::load();
        // An index saved under other rules still answers until its places are read again.
        let cached = Index::load().map(|(index, _)| index).unwrap_or_default();
        let index = Arc::new(RwLock::new(cached));
        let status = Arc::new(IndexStatus { walking: AtomicBool::new(false), walked: AtomicUsize::new(0), last_walk_ms: AtomicU64::new(0) });
        let (tx, rx) = mpsc::channel::<Cmd>();
        let manager = IndexManager { index: index.clone(), status: status.clone(), tx: tx.clone() };
        let _ = tx.send(Cmd::Reindex(params));
        std::thread::Builder::new()
            .name("index".into())
            .spawn(move || {
                // Folders a watch reports are read on this thread: background mode here too.
                platform::background_mode();
                let mut keeper = Keeper {
                    index,
                    status,
                    tx,
                    active: None,
                    rules: Rules::default(),
                    kinds: HashMap::new(),
                    log,
                    watchers: Vec::new(),
                    pending: Vec::new(),
                    walking: Vec::new(),
                    due: Vec::new(),
                    due_idle: Vec::new(),
                    dirty: false,
                    last_save: Instant::now(),
                };
                loop {
                    match rx.recv_timeout(Duration::from_secs(60)) {
                        Ok(Cmd::Reindex(p)) => keeper.reindex(p),
                        Ok(Cmd::Change(change)) => {
                            let Some(p) = keeper.active.clone() else { continue };
                            if let Ok(mut live) = keeper.index.write() {
                                live.apply(&change, &p, &keeper.rules);
                            }
                            keeper.dirty = true;
                            if !keeper.walking.is_empty() {
                                keeper.pending.push(change);
                                // A huge burst (an unpacked archive, say): the walk may have seen some of it, so its
                                // places are read once more afterwards rather than replaying thousands of events.
                                if keeper.pending.len() > 20_000 {
                                    keeper.pending.clear();
                                    for root in keeper.walking.clone() {
                                        push_once(&mut keeper.due, &root);
                                    }
                                }
                            }
                        }
                        Ok(Cmd::Overflow(root)) => {
                            // Too much changed at once for Windows to list: this place is read again, and remembered
                            // as such in case Startline closes first.
                            push_once(&mut keeper.log.overflowed, &root);
                            keeper.log.save();
                            push_once(&mut keeper.due, &root);
                            keeper.start_due();
                        }
                        Ok(Cmd::WalkDone(finished, roots)) => keeper.walk_done(finished, roots),
                        Err(RecvTimeoutError::Timeout) => keeper.tick(),
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

    const NOW: u64 = 1_800_000_000;

    #[test]
    fn a_watched_place_is_read_only_when_it_has_to_be() {
        let fresh = Some(NOW - 3600);
        let old = Some(NOW - DAY - 1);
        for kind in [PlaceKind::Hdd, PlaceKind::Ssd, PlaceKind::Usb] {
            assert_eq!(plan(kind, true, fresh, false, false, NOW), Plan::Keep, "{kind:?}: the saved index and the watch are trusted");
            assert_eq!(plan(kind, false, fresh, false, false, NOW), Plan::Walk, "{kind:?}: not in the index yet");
            assert_eq!(plan(kind, true, None, false, false, NOW), Plan::Walk, "{kind:?}: never read, the first run");
            assert_eq!(plan(kind, true, fresh, true, false, NOW), Plan::Walk, "{kind:?}: what's left out changed");
            assert_eq!(plan(kind, true, fresh, false, true, NOW), Plan::Walk, "{kind:?}: the watch overflowed");
            assert_eq!(plan(kind, true, old, false, false, NOW), Plan::Walk, "{kind:?}: more than a day old");
        }
    }

    #[test]
    fn a_network_share_is_read_at_most_once_a_day_and_only_when_idle() {
        let net = PlaceKind::Network;
        assert_eq!(plan(net, true, Some(NOW - 3600), false, false, NOW), Plan::Keep);
        assert_eq!(plan(net, true, Some(NOW - 3600), false, true, NOW), Plan::Keep, "no watch, so no overflow either");
        assert_eq!(plan(net, true, Some(NOW - DAY), false, false, NOW), Plan::WalkWhenIdle);
        assert_eq!(plan(net, false, None, false, false, NOW), Plan::WalkWhenIdle, "never read: only when idle too");
        assert!(!net.watched() && net.threads() == 1);
        assert!(PlaceKind::Hdd.threads() == 1 && PlaceKind::Usb.threads() == 1 && PlaceKind::Ssd.threads() == 4);
    }

    #[test]
    fn a_slow_place_keeps_its_pace() {
        let gate = Gate { pause: false, per_second: Some(20), last: Mutex::new(None), checked: Mutex::new(None) };
        let started = Instant::now();
        for _ in 0..5 {
            gate.before_listing();
        }
        assert!(started.elapsed() >= Duration::from_millis(190), "five listings at 20 a second take at least 200 ms: {:?}", started.elapsed());
        let free = Gate::none();
        let started = Instant::now();
        for _ in 0..50 {
            free.before_listing();
        }
        assert!(started.elapsed() < Duration::from_millis(50));
    }

    #[test]
    fn a_walk_waits_on_battery_only_for_a_while() {
        let minutes = |m: u64| Duration::from_secs(m * 60);
        assert!(!waits(false, Duration::ZERO, false), "on mains with nothing in front, it reads");
        assert!(waits(true, Duration::ZERO, false), "just gone on battery: it waits");
        assert!(waits(true, minutes(9), false));
        assert!(!waits(true, minutes(10), false), "ten minutes on battery: it carries on");
        assert!(!waits(true, minutes(120), false));
        assert!(waits(false, Duration::ZERO, true), "a full-screen app in front: it waits");
        assert!(waits(true, minutes(120), true), "for as long as it's in front, battery or not");
    }

    #[test]
    fn a_walk_log_survives_saving() {
        let log = WalkLog { rules: 7, walked: [("C:\\Users\\me".to_string(), NOW)].into(), overflowed: vec!["D:\\".into()] };
        let text = serde_json::to_string(&log).unwrap();
        assert_eq!(serde_json::from_str::<WalkLog>(&text).unwrap(), log);
        assert_eq!(serde_json::from_str::<WalkLog>("{}").unwrap(), WalkLog::default(), "an older or missing file reads as never walked");
    }
}
