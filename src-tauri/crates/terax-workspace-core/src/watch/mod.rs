//! Live directory change notifications, independent of the app shell.
//!
//! Directories are registered in a [`plan::WatchSet`]; an OS backend reports
//! raw events into a [`Sink`]; one drain thread debounces them, re-reads the
//! touched listings and reports only what moved. The app supplies where the
//! results go ([`Reporter`]) and what to invalidate ([`ChangeNotes`]); the
//! remote agent will run the same engine on the server.

pub mod plan;
#[cfg(not(target_os = "macos"))]
pub mod portable;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use crate::paths::to_canon;
use plan::{Batch, DirSet, FileStamp, ListingSnapshot, ListingStamp, Pending, WatchSet};

// Quiet-gap before a batch flushes; MAX_WINDOW caps latency under a long stream.
const DEBOUNCE: Duration = Duration::from_millis(150);
const MAX_WINDOW: Duration = Duration::from_millis(1000);

pub trait Backend: Send {
    /// Brings the OS watches in line with `set`. Called after every change to
    /// the set; implementations must be a no-op when nothing relevant moved.
    fn sync(&mut self, set: &WatchSet);
}

/// Told about every path a change touched, so caches built from the tree
/// (the file finder's index) can drop what went stale.
pub trait ChangeNotes: Send + Sync {
    fn note_change(&self, path: &str);
}

/// Where a burst's results go.
pub trait Reporter: Send + Sync {
    /// Every watched directory the burst touched, before the listing filter,
    /// so in-place file edits are included. Runs on the drain thread: it must
    /// only hand the paths off.
    fn touched(&self, dirs: &[String]);
    /// Watched directories whose listing moved, plus registered files whose
    /// stamp moved: what the UI needs to refresh.
    fn changed(&self, paths: Vec<String>);
}

/// What a backend callback needs: the live set of watched directories to
/// filter against, a way to report a batch, and the change notes.
#[derive(Clone)]
pub struct Sink {
    pub tx: mpsc::Sender<Batch>,
    pub dirs: Arc<RwLock<Arc<DirSet>>>,
    pub notes: Arc<dyn ChangeNotes>,
}

impl Sink {
    pub fn send(&self, batch: Batch) {
        if !batch.is_empty() {
            let _ = self.tx.send(batch);
        }
    }

    pub fn dirs(&self) -> Arc<DirSet> {
        self.dirs.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Any directory-level change, watched or not, can stale a file list.
    pub fn note_path(&self, path: &[u8]) {
        if let Ok(text) = std::str::from_utf8(path) {
            self.notes.note_change(text);
        }
    }
}

struct Shared {
    set: Mutex<WatchSet>,
    dirs: Arc<RwLock<Arc<DirSet>>>,
}

/// A running watcher: the watched set, the OS backend, and the queue its
/// events go through.
pub struct Watcher {
    shared: Arc<Shared>,
    backend: Box<dyn Backend>,
    tx: mpsc::Sender<Batch>,
}

impl Watcher {
    /// Starts the drain thread and the backend `make_backend` builds around
    /// the sink it is given.
    pub fn start(
        notes: Arc<dyn ChangeNotes>,
        reporter: Arc<dyn Reporter>,
        make_backend: impl FnOnce(Sink) -> Result<Box<dyn Backend>, String>,
    ) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel::<Batch>();
        let shared = Arc::new(Shared {
            set: Mutex::new(WatchSet::default()),
            dirs: Arc::new(RwLock::new(Arc::new(DirSet::new()))),
        });
        let sink = Sink {
            tx: tx.clone(),
            dirs: shared.dirs.clone(),
            notes: notes.clone(),
        };
        let backend = make_backend(sink)?;

        let drain_shared = shared.clone();
        std::thread::Builder::new()
            .name("terax-fs-watch".into())
            .stack_size(256 * 1024)
            .spawn(move || drain_loop(rx, drain_shared, notes, reporter))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            shared,
            backend,
            tx,
        })
    }
}

fn drain_loop(
    rx: mpsc::Receiver<Batch>,
    shared: Arc<Shared>,
    notes: Arc<dyn ChangeNotes>,
    reporter: Arc<dyn Reporter>,
) {
    loop {
        let Ok(first) = rx.recv() else { return };
        let mut pending = Pending::default();
        pending.merge(first);

        let deadline = Instant::now() + MAX_WINDOW;
        loop {
            let timeout = DEBOUNCE.min(deadline.saturating_duration_since(Instant::now()));
            match rx.recv_timeout(timeout) {
                Ok(batch) => pending.merge(batch),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if Instant::now() >= deadline {
                break;
            }
        }

        let Changes { touched, report } = resolve_changes(pending, &shared);
        // The file list cache hears about every touched directory, whether or not the
        // frontend does: an edited ignore file changes the list without changing a name.
        for path in &touched {
            notes.note_change(path);
        }
        if !touched.is_empty() {
            reporter.touched(&touched);
        }
        if report.is_empty() {
            continue;
        }
        reporter.changed(report);
    }
}

/// What one burst resolved to.
struct Changes {
    /// Watched directories the backend reported.
    touched: Vec<String>,
    /// What the frontend is told: the touched directories whose listing moved, plus registered
    /// files inside them whose stamp moved.
    report: Vec<String>,
}

/// Watched directories that changed, plus registered files inside them whose
/// stamp moved. A directory whose listing is the one the frontend already has (a file edited
/// in place, a build rewriting a file) is left out: see `WatchSet::listing_changed`.
fn resolve_changes(pending: Pending, shared: &Shared) -> Changes {
    resolve_changes_with(pending, shared, &plan::listing_signature)
}

fn resolve_changes_with(
    pending: Pending,
    shared: &Shared,
    scan: &dyn Fn(&std::path::Path) -> Option<u64>,
) -> Changes {
    let (touched, locations) = {
        let set = shared.set.lock().unwrap_or_else(|e| e.into_inner());
        let touched = pending.resolve(set.dir_keys());
        let locations: Vec<_> = touched
            .iter()
            .map(|dir| {
                (
                    set.dir_path(dir),
                    set.listing_stamp(dir),
                    pending.listing_change(dir),
                )
            })
            .collect();
        (touched, locations)
    };
    // Reading the directories happens outside the lock: add/remove never wait for the disk.
    let snapshots: Vec<_> = locations
        .iter()
        .map(|(path, previous, change)| {
            let path = path.as_deref()?;
            if !change.forced
                && ListingStamp::of(path)
                    .zip(*previous)
                    .is_some_and(|(now, before)| now.can_skip(&before, change.content_only))
            {
                return None;
            }
            Some(ListingSnapshot::read(path, scan))
        })
        .collect();
    let mut set = shared.set.lock().unwrap_or_else(|e| e.into_inner());
    let stamp_of = |path: &str| FileStamp::of(std::path::Path::new(path));
    let mut report = Vec::new();
    for (dir, snapshot) in touched.iter().zip(snapshots) {
        if let Some(snapshot) = snapshot {
            if set.listing_changed(dir, snapshot.signature) {
                report.push(dir.clone());
            }
            set.record_listing_stamp(dir, snapshot.stamp);
        }
        report.extend(set.changed_files_in(dir, &stamp_of));
    }
    Changes { touched, report }
}

impl Watcher {
    pub fn add(
        &mut self,
        dirs: impl IntoIterator<Item = (PathBuf, ListingSnapshot)>,
        files: impl IntoIterator<Item = (String, String, Option<FileStamp>)>,
    ) {
        let mut recheck = HashSet::new();
        let mut file_recheck = HashSet::new();
        self.apply(|set| {
            let mut moved = false;
            for (dir, baseline) in dirs {
                let key = to_canon(&dir);
                if set.add_dir(key.clone(), dir) {
                    set.record_listing(&key, baseline.signature);
                    set.record_listing_stamp(&key, baseline.stamp);
                    recheck.insert(key);
                    moved = true;
                }
            }
            for (dir, key, stamp) in files {
                if set.add_file(dir.clone(), key, stamp) && !recheck.contains(&dir) {
                    file_recheck.insert(dir);
                }
            }
            moved
        });
        // Changes between baseline capture and OS registration have no callback.
        // Reconcile after sync, through the same queue as subsequent native events.
        for (dirs, forced) in [(recheck, true), (file_recheck, false)] {
            if !dirs.is_empty() {
                let _ = self.tx.send(Batch {
                    dirs: dirs.into_iter().collect(),
                    listing: plan::ListingChange {
                        forced,
                        content_only: !forced,
                    },
                    ..Batch::default()
                });
            }
        }
    }

    /// `change` reports whether the set of watched directories moved; file
    /// bookkeeping alone never touches the OS watches.
    pub fn apply(&mut self, change: impl FnOnce(&mut WatchSet) -> bool) {
        let mut set = self.shared.set.lock().unwrap_or_else(|e| e.into_inner());
        if !change(&mut set) {
            return;
        }
        *self.shared.dirs.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(set.snapshot());
        self.backend.sync(&set);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RegistrationBackend(Box<dyn FnMut() + Send>);

    impl Backend for RegistrationBackend {
        fn sync(&mut self, _: &WatchSet) {
            (self.0)();
        }
    }

    fn registration(on_sync: impl FnMut() + Send + 'static) -> (Watcher, mpsc::Receiver<Batch>) {
        let (tx, rx) = mpsc::channel();
        (
            Watcher {
                shared: Arc::new(Shared {
                    set: Mutex::new(WatchSet::default()),
                    dirs: Arc::new(RwLock::new(Arc::new(DirSet::new()))),
                }),
                backend: Box::new(RegistrationBackend(Box::new(on_sync))),
                tx,
            },
            rx,
        )
    }

    #[test]
    fn registration_reconciles_changes_without_a_backend_event() {
        for operation in ["create", "delete", "rename", "edit"] {
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("before.txt");
            std::fs::write(&file, "before").unwrap();
            let baseline = ListingSnapshot::read(dir.path(), &plan::listing_signature);
            let file_stamp = FileStamp::of(&file);
            let root = dir.path().to_path_buf();
            let (mut running, rx) = registration(move || {
                match operation {
                    "create" => std::fs::write(root.join("after.txt"), "new").unwrap(),
                    "delete" => std::fs::remove_file(root.join("before.txt")).unwrap(),
                    "rename" => {
                        std::fs::rename(root.join("before.txt"), root.join("after.txt")).unwrap()
                    }
                    "edit" => {
                        std::fs::write(root.join("before.txt"), "changed during registration")
                            .unwrap()
                    }
                    _ => unreachable!(),
                }
                // The OS watch is installed only after this change; no callback is emitted.
            });
            running.add(
                [(dir.path().to_path_buf(), baseline)],
                [(to_canon(dir.path()), to_canon(&file), file_stamp)],
            );
            let mut pending = Pending::default();
            pending.merge(
                rx.try_recv()
                    .expect("registration must queue a reconciliation"),
            );
            let changes = resolve_changes(pending, &running.shared);
            let expected = if operation == "edit" {
                to_canon(&file)
            } else {
                to_canon(dir.path())
            };
            assert!(
                changes.report.contains(&expected),
                "{operation}: {:?}",
                changes.report
            );
            assert_eq!(changes.touched, vec![to_canon(dir.path())]);
        }
    }

    #[test]
    fn registration_without_changes_is_quiet_and_duplicates_do_not_rescan() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file.txt");
        std::fs::write(&file, "unchanged").unwrap();
        let syncs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = syncs.clone();
        let (mut running, rx) = registration(move || {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        for index in 0..2 {
            running.add(
                [(
                    dir.path().to_path_buf(),
                    ListingSnapshot::read(dir.path(), &plan::listing_signature),
                )],
                [(to_canon(dir.path()), to_canon(&file), FileStamp::of(&file))],
            );
            if index == 0 {
                let mut pending = Pending::default();
                let batch = rx.try_recv().unwrap();
                assert_eq!(
                    batch.dirs.len(),
                    1,
                    "the file and directory share one check"
                );
                assert!(batch.listing.forced);
                pending.merge(batch);
                assert!(resolve_changes(pending, &running.shared).report.is_empty());
            }
            assert!(
                rx.try_recv().is_err(),
                "existing subscriptions need no new reconciliation"
            );
        }
        assert_eq!(syncs.load(std::sync::atomic::Ordering::SeqCst), 1);
        let mut set = running.shared.set.lock().unwrap();
        assert!(!set.remove_dir(&to_canon(dir.path())));
        set.remove_file(&to_canon(dir.path()), &to_canon(&file));
        std::fs::write(&file, "still watched after one unsubscribe").unwrap();
        assert_eq!(
            set.changed_files_in(&to_canon(dir.path()), &|path| FileStamp::of(
                std::path::Path::new(path)
            )),
            vec![to_canon(&file)]
        );
        assert!(set.remove_dir(&to_canon(dir.path())));
        set.remove_file(&to_canon(dir.path()), &to_canon(&file));
        assert!(set.is_empty());
    }

    #[test]
    fn a_new_file_in_an_existing_directory_is_reconciled() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file.txt");
        std::fs::write(&file, "before").unwrap();
        let syncs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = syncs.clone();
        let (mut running, rx) = registration(move || {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        running.add(
            [(
                dir.path().to_path_buf(),
                ListingSnapshot::read(dir.path(), &plan::listing_signature),
            )],
            [],
        );
        let mut initial = Pending::default();
        initial.merge(rx.try_recv().unwrap());
        assert!(resolve_changes(initial, &running.shared).report.is_empty());
        let before = FileStamp::of(&file);
        // An existing watch may already have delivered this write before add_file.
        std::fs::write(&file, "changed before file registration").unwrap();
        running.add([], [(to_canon(dir.path()), to_canon(&file), before)]);
        let mut pending = Pending::default();
        pending.merge(
            rx.try_recv()
                .expect("new files need reconciliation even without a backend sync"),
        );
        assert_eq!(
            resolve_changes_with(pending, &running.shared, &|_| {
                panic!("a file-only registration must not enumerate its directory")
            })
            .report,
            vec![to_canon(&file)]
        );
        assert_eq!(syncs.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    fn shared_watching(dir: &std::path::Path) -> Shared {
        let key = to_canon(dir);
        let mut set = WatchSet::default();
        assert!(set.add_dir(key.clone(), dir.to_path_buf()));
        let baseline = ListingSnapshot::read(dir, &plan::listing_signature);
        set.record_listing(&key, baseline.signature);
        set.record_listing_stamp(&key, baseline.stamp);
        Shared {
            set: Mutex::new(set),
            dirs: Arc::new(RwLock::new(Arc::new(DirSet::new()))),
        }
    }

    fn burst(dir: &std::path::Path) -> Pending {
        let mut pending = Pending::default();
        pending.merge(Batch {
            dirs: vec![to_canon(dir)],
            subtrees: Vec::new(),
            ..Batch::default()
        });
        pending
    }

    #[test]
    fn repeated_content_edits_do_not_enumerate_a_wide_directory() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..1000 {
            std::fs::write(dir.path().join(format!("{i}.txt")), "").unwrap();
        }
        let shared = shared_watching(dir.path());
        let reusable = shared
            .set
            .lock()
            .unwrap()
            .listing_stamp(&to_canon(dir.path()))
            .is_some_and(|stamp| stamp.can_skip(&stamp, false));
        let expected_scans = if reusable { 0 } else { 9 };
        let scans = std::cell::Cell::new(0);
        let scan = |path: &std::path::Path| {
            scans.set(scans.get() + 1);
            plan::listing_signature(path)
        };
        for i in 1..10 {
            std::fs::write(dir.path().join("0.txt"), "x".repeat(i)).unwrap();
            let changes = resolve_changes_with(burst(dir.path()), &shared, &scan);
            assert!(changes.report.is_empty());
        }
        assert_eq!(
            scans.get(),
            expected_scans,
            "only uncertain directory metadata should require a scan"
        );
        std::fs::rename(dir.path().join("0.txt"), dir.path().join("renamed.txt")).unwrap();
        let changes = resolve_changes_with(burst(dir.path()), &shared, &scan);
        assert_eq!(changes.report, vec![to_canon(dir.path())]);
        assert_eq!(scans.get(), expected_scans + 1);
    }

    #[test]
    fn content_only_batches_skip_listing_but_still_report_registered_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("open.txt");
        std::fs::write(&file, "old").unwrap();
        let shared = shared_watching(dir.path());
        shared.set.lock().unwrap().add_file(
            to_canon(dir.path()),
            to_canon(&file),
            FileStamp::of(&file),
        );
        std::fs::write(&file, "new content").unwrap();
        let mut pending = Pending::default();
        pending.merge(Batch {
            dirs: vec![to_canon(dir.path())],
            listing: plan::ListingChange {
                content_only: true,
                forced: false,
            },
            ..Batch::default()
        });
        let changes = resolve_changes_with(pending, &shared, &|_| {
            panic!("content-only event must not enumerate")
        });
        assert_eq!(changes.report, vec![to_canon(&file)]);
    }

    #[cfg(unix)]
    #[test]
    fn restoring_directory_mtime_cannot_hide_a_rename() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("before.txt"), "").unwrap();
        let directory = std::fs::File::open(dir.path()).unwrap();
        let original =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::new(123456, 123456789);
        directory.set_modified(original).unwrap();
        let shared = shared_watching(dir.path());
        // Give the filesystem change clock a tick; the mtime is restored below.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::rename(dir.path().join("before.txt"), dir.path().join("after.txt")).unwrap();
        directory.set_modified(original).unwrap();
        let changes = resolve_changes(burst(dir.path()), &shared);
        assert_eq!(changes.report, vec![to_canon(dir.path())]);
    }

    #[test]
    fn content_edits_through_an_ignore_alias_still_update_the_listing() {
        let dir = tempfile::tempdir().unwrap();
        let ignore = dir.path().join(".gitignore");
        let alias = dir.path().join("rules.txt");
        std::fs::write(&ignore, "old").unwrap();
        std::fs::hard_link(&ignore, &alias).unwrap();
        let shared = shared_watching(dir.path());
        std::fs::write(&alias, "a new ignore rule").unwrap();
        let mut pending = Pending::default();
        pending.merge(Batch {
            dirs: vec![to_canon(dir.path())],
            listing: plan::ListingChange {
                content_only: true,
                forced: false,
            },
            ..Batch::default()
        });
        let changes = resolve_changes(pending, &shared);
        assert_eq!(changes.report, vec![to_canon(dir.path())]);
    }

    #[test]
    fn ignore_edits_and_uncertain_events_still_scan_the_listing() {
        let dir = tempfile::tempdir().unwrap();
        let ignore = dir.path().join(".gitignore");
        std::fs::write(&ignore, "old").unwrap();
        let shared = shared_watching(dir.path());
        let scans = std::cell::Cell::new(0);
        let scan = |path: &std::path::Path| {
            scans.set(scans.get() + 1);
            plan::listing_signature(path)
        };
        std::fs::write(&ignore, "a new ignore rule").unwrap();
        let changed = resolve_changes_with(burst(dir.path()), &shared, &scan);
        assert_eq!(changed.report, vec![to_canon(dir.path())]);
        assert_eq!(scans.get(), 1);
        let mut dropped = Pending::default();
        dropped.merge(Batch {
            subtrees: vec![to_canon(dir.path())],
            ..Batch::default()
        });
        resolve_changes_with(dropped, &shared, &scan);
        assert_eq!(
            scans.get(),
            2,
            "dropped events force a scan even with stable metadata"
        );
    }

    #[test]
    fn edits_in_place_reach_the_index_but_not_the_frontend() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.ts"), "one").unwrap();
        let shared = shared_watching(dir.path());

        std::fs::write(dir.path().join("a.ts"), "two, longer").unwrap();
        let changes = resolve_changes(burst(dir.path()), &shared);
        assert_eq!(changes.touched, vec![to_canon(dir.path())]);
        assert!(
            changes.report.is_empty(),
            "nothing the explorer shows moved"
        );

        std::fs::write(dir.path().join("b.ts"), "").unwrap();
        let changes = resolve_changes(burst(dir.path()), &shared);
        assert_eq!(changes.report, vec![to_canon(dir.path())]);

        let changes = resolve_changes(burst(dir.path()), &shared);
        assert!(changes.report.is_empty(), "the new listing is now known");

        std::fs::remove_file(dir.path().join("b.ts")).unwrap();
        let changes = resolve_changes(burst(dir.path()), &shared);
        assert_eq!(changes.report, vec![to_canon(dir.path())]);
    }

    #[test]
    fn a_registered_file_is_still_reported_when_only_its_contents_change() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("open.ts");
        std::fs::write(&file, "one").unwrap();
        let shared = shared_watching(dir.path());
        shared.set.lock().unwrap().add_file(
            to_canon(dir.path()),
            to_canon(&file),
            FileStamp::of(&file),
        );

        std::fs::write(&file, "two, a different length").unwrap();
        let changes = resolve_changes(burst(dir.path()), &shared);
        assert_eq!(changes.report, vec![to_canon(&file)]);
    }
}
