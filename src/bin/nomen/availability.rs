//! Background availability work: the bulk snapshot, and single re-checks.

use std::sync::mpsc::{self, Receiver};
use std::thread;

use nomen::{Availability, Registry, Snapshot};

pub enum SnapshotMsg {
    Downloading,
    Ready(Snapshot),
    Failed(String),
}

/// Load the stored snapshot, refreshing it if it is missing or past its age.
///
/// Runs off the UI thread so a first launch stays usable while ~356 MB
/// downloads.
pub fn spawn_snapshot(force: bool) -> Receiver<SnapshotMsg> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let existing = Snapshot::load();
        let need = force || existing.as_ref().is_none_or(|s| s.is_stale());

        if let Some(snap) = existing
            && !force
        {
            let stale = snap.is_stale();
            let _ = tx.send(SnapshotMsg::Ready(snap));
            if !stale {
                return;
            }
        } else if need {
            let _ = tx.send(SnapshotMsg::Downloading);
        }

        if need {
            match Snapshot::fetch() {
                Ok(snap) => {
                    let _ = snap.save();
                    let _ = tx.send(SnapshotMsg::Ready(snap));
                }
                Err(e) => {
                    let _ = tx.send(SnapshotMsg::Failed(e.to_string()));
                }
            }
        }
    });
    rx
}

/// Ask crates.io about one name directly, bypassing the snapshot.
//
// the snapshot is up to a week old; this is the escape hatch when a single
// name matters enough to want a current answer
pub fn spawn_recheck(row: usize, name: String) -> Receiver<(usize, Availability)> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let status = Registry::new().check(&[name.as_str()]).first().copied();
        let _ = tx.send((row, status.unwrap_or(Availability::Unknown)));
    });
    rx
}
