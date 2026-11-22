//! End to end through the real OS backend: a change under a watched
//! directory comes out of the engine as one debounced report.
#![cfg(not(target_os = "macos"))]

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use terax_workspace_core::paths::to_canon;
use terax_workspace_core::watch::plan::ListingSnapshot;
use terax_workspace_core::watch::portable::Notify;
use terax_workspace_core::watch::{plan, ChangeNotes, Reporter, Watcher};

#[derive(Default)]
struct Notes(Mutex<Vec<String>>);

impl ChangeNotes for Notes {
    fn note_change(&self, path: &str) {
        self.0.lock().unwrap().push(path.to_string());
    }
}

struct Channel(Mutex<mpsc::Sender<Vec<String>>>);

impl Reporter for Channel {
    fn touched(&self, _: &[String]) {}

    fn changed(&self, paths: Vec<String>) {
        let _ = self.0.lock().unwrap().send(paths);
    }
}

#[test]
fn a_new_file_in_a_watched_directory_is_reported_once_and_noted() {
    let dir = tempfile::tempdir().unwrap();
    let canonical = std::fs::canonicalize(dir.path()).unwrap();
    let key = to_canon(&canonical);
    let notes = Arc::new(Notes::default());
    let (tx, rx) = mpsc::channel();
    let mut watcher = Watcher::start(notes.clone(), Arc::new(Channel(Mutex::new(tx))), |sink| {
        Ok(Box::new(Notify::new(sink)?))
    })
    .unwrap();

    let baseline = ListingSnapshot::read(&canonical, &plan::listing_signature);
    watcher.add([(canonical.clone(), baseline)], []);
    // Registration reconciles once against the baseline; nothing changed yet.
    assert!(
        rx.recv_timeout(Duration::from_millis(600)).is_err(),
        "an unchanged directory is not reported"
    );

    std::fs::write(canonical.join("new.txt"), "x").unwrap();
    let report = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("a created file is reported");
    assert_eq!(report, std::slice::from_ref(&key));
    assert!(
        rx.recv_timeout(Duration::from_millis(600)).is_err(),
        "one burst, one report"
    );
    assert!(
        notes.0.lock().unwrap().contains(&key),
        "the touched directory reaches the change notes"
    );
}
