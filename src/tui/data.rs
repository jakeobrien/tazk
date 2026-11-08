//! `Snapshot` (issues including closed, dependents, `by_id` index, triage items,
//! skipped files) and loading from storage.

use crate::storage::triage::TriageItem;
use crate::storage::{closed_issue_ids, SkippedFile, Storage};
use crate::types::Issue;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

/// An immutable view of everything on disk at one moment (spec §3.1).
///
/// The app replaces the whole snapshot on every reload.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    /// Every issue, including closed ones, with `dependents` populated.
    pub issues: Vec<Issue>,
    /// Issue ID -> index into `issues`.
    pub by_id: HashMap<String, usize>,
    /// IDs of closed issues, as the storage helper for `tz ready` wants them.
    pub closed_ids: HashSet<String>,
    /// Triage items, sorted by file name.
    pub triage: Vec<TriageItem>,
    /// Files that failed to load (spec §3.2).
    pub skipped: Vec<SkippedFile>,
}

impl Snapshot {
    /// Build a snapshot (and its `by_id` index) from already-loaded parts.
    /// No filesystem access.
    pub fn new(issues: Vec<Issue>, triage: Vec<TriageItem>, skipped: Vec<SkippedFile>) -> Self {
        let by_id = issues
            .iter()
            .enumerate()
            .map(|(i, issue)| (issue.id.clone(), i))
            .collect();
        let closed_ids = closed_issue_ids(&issues);
        Self {
            issues,
            by_id,
            closed_ids,
            triage,
            skipped,
        }
    }

    /// Load everything with the tolerant loaders: bad files are skipped and
    /// reported, never fatal. Only takes short read locks.
    pub fn load(storage: &Storage) -> Result<Self> {
        let issues = storage.list_all_issues_tolerant()?;
        let triage = storage.list_triage_items_tolerant()?;
        let mut skipped = issues.skipped;
        skipped.extend(triage.skipped);
        Ok(Self::new(issues.items, triage.items, skipped))
    }

    pub fn issue(&self, id: &str) -> Option<&Issue> {
        self.by_id.get(id).map(|&i| &self.issues[i])
    }
}

/// Produces a snapshot; run on a background thread by [`Reloader`].
pub type LoadFn = Arc<dyn Fn() -> Result<Snapshot> + Send + Sync>;

/// Runs loads on a background thread so input stays responsive (spec §3.3).
///
/// At most one load is in flight. A request that arrives meanwhile is
/// coalesced into a single follow-up load, started when the current one
/// finishes (so a change made during a slow load is still picked up).
/// Results are collected with [`Reloader::poll`] by the event loop.
pub struct Reloader {
    load: LoadFn,
    tx: Sender<Result<Snapshot, String>>,
    rx: Receiver<Result<Snapshot, String>>,
    in_flight: bool,
    pending: bool,
}

impl Reloader {
    pub fn new(load: LoadFn) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            load,
            tx,
            rx,
            in_flight: false,
            pending: false,
        }
    }

    /// A reloader that loads from `storage` with [`Snapshot::load`].
    pub fn for_storage(storage: Storage) -> Self {
        Self::for_shared(Arc::new(storage))
    }

    /// Like [`Reloader::for_storage`], for a storage shared with the caller.
    pub fn for_shared(storage: Arc<Storage>) -> Self {
        Self::new(Arc::new(move || Snapshot::load(&storage)))
    }

    /// Whether a load is running.
    pub fn in_flight(&self) -> bool {
        self.in_flight
    }

    /// Ask for a reload. Never blocks.
    pub fn request(&mut self) {
        if self.in_flight {
            self.pending = true;
            return;
        }
        self.in_flight = true;
        let load = Arc::clone(&self.load);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| load()))
                .unwrap_or_else(|_| Err(anyhow::anyhow!("loader panicked")))
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(result);
        });
    }

    /// A finished load, if any (never blocks). Starts the coalesced
    /// follow-up load if one was requested meanwhile.
    pub fn poll(&mut self) -> Option<Result<Snapshot, String>> {
        let result = self.rx.try_recv().ok()?;
        self.in_flight = false;
        if std::mem::take(&mut self.pending) {
            self.request();
        }
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::{self, Action, App};
    use crate::tui::terminal::handle_event;
    use crate::tui::testing::*;
    use crate::tui::ModeArg;
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    fn wait(r: &mut Reloader) -> Result<Snapshot, String> {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(res) = r.poll() {
                return res;
            }
            assert!(Instant::now() < end, "loader never finished");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn a_slow_loader_does_not_block_key_handling() {
        let (release, gate) = mpsc::channel::<()>();
        let gate = std::sync::Mutex::new(gate);
        let mut r = Reloader::new(Arc::new(move || {
            gate.lock().unwrap().recv().unwrap();
            Ok(Snapshot::new(
                vec![issue("tz-9", "Late").build()],
                vec![],
                vec![],
            ))
        }));
        let mut a = App::from_snapshot(ModeArg::Issues, snapshot());
        r.request();
        assert!(r.in_flight());
        assert!(r.poll().is_none());
        // Keys are still handled while the load is stuck.
        handle_event(
            &mut a,
            Event::Key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE)),
        );
        assert_eq!(a.mode, ModeArg::Triage);
        release.send(()).unwrap();
        let snap = wait(&mut r).unwrap();
        assert!(!r.in_flight());
        app::update(&mut a, Action::Reloaded(Box::new(snap)));
        assert!(a.snapshot.issue("tz-9").is_some());
    }

    #[test]
    fn overlapping_requests_coalesce_into_one_follow_up_load() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (release, gate) = mpsc::channel::<()>();
        let gate = std::sync::Mutex::new(gate);
        let c = Arc::clone(&calls);
        let mut r = Reloader::new(Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
            gate.lock().unwrap().recv().unwrap();
            Ok(Snapshot::default())
        }));
        for _ in 0..5 {
            r.request();
        }
        release.send(()).unwrap();
        release.send(()).unwrap();
        wait(&mut r).unwrap();
        // The follow-up load is now in flight.
        assert!(r.in_flight());
        wait(&mut r).unwrap();
        assert!(!r.in_flight());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(r.poll().is_none());
    }

    #[test]
    fn load_errors_and_panics_are_reported_and_free_the_slot() {
        let mut r = Reloader::new(Arc::new(|| Err(anyhow::anyhow!("disk gone"))));
        r.request();
        assert_eq!(wait(&mut r).unwrap_err(), "disk gone");
        let mut r = Reloader::new(Arc::new(|| panic!("boom")));
        r.request();
        assert!(wait(&mut r).is_err());
        assert!(!r.in_flight());
    }

    #[test]
    fn storage_reload_updates_the_skipped_count() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = Storage::open(tmp.path().join(".tazk")).unwrap();
        let issues = tmp.path().join(".tazk/issues");
        let mut r = Reloader::for_storage(storage);
        r.request();
        assert_eq!(wait(&mut r).unwrap().skipped.len(), 0);
        std::fs::write(issues.join("tz-1.md"), "---\n: not [yaml\n---\n").unwrap();
        r.request();
        let snap = wait(&mut r).unwrap();
        assert_eq!(snap.skipped.len(), 1);
        let mut a = App::from_snapshot(ModeArg::Issues, Snapshot::default());
        app::update(&mut a, Action::Reloaded(Box::new(snap)));
        assert_eq!(a.snapshot.skipped.len(), 1);
    }

    #[test]
    fn new_builds_index() {
        let s = snapshot();
        assert_eq!(s.by_id["tz-2"], 1);
        assert_eq!(s.issue("tz-1").unwrap().title, "First");
        assert!(s.issue("nope").is_none());
        assert_eq!(s.triage.len(), 1);
    }
}
