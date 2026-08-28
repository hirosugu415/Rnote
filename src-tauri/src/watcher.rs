//! Keeps the in-memory note cache honest against changes made outside the
//! app — editing a `.md` file directly in Notepad, or dragging one into
//! `keep/` from Explorer.
//!
//! Unlike Rfinder's watcher (which patches its index path by path, because a
//! whole-drive rescan would be far too slow to redo per batch), this just
//! rescans both folders whole on every debounced batch. At a personal note
//! count — hundreds, maybe low thousands of small text files — a full
//! `notes::scan::scan_all` finishes well inside the debounce window itself,
//! so there is nothing to gain from the extra bookkeeping a partial patch
//! would need.

use crate::config::Status;
use crate::notes::scan;
use crate::state::AppState;
use notify_debouncer_full::notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer_opt, DebounceEventResult, NoCache};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const DEBOUNCE: Duration = Duration::from_millis(500);
/// How often the loop wakes up even without an event, just to notice a
/// generation bump (a `notesDir` change asking this watcher to retire).
const GENERATION_POLL: Duration = Duration::from_secs(2);

pub const NOTES_CHANGED: &str = "notes-changed";

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let notes_dir = state.config.read().notes_dir.clone();
        let watch_generation = state.watch_generation.clone();
        drop(state);
        let generation = watch_generation.load(Ordering::SeqCst);

        let keep_dir = notes_dir.join(Status::Keep.dirname());
        let scratch_dir = notes_dir.join(Status::Scratch.dirname());

        let (tx, rx) = channel::<DebounceEventResult>();
        let debouncer = new_debouncer_opt::<_, RecommendedWatcher, NoCache>(
            DEBOUNCE,
            None,
            tx,
            NoCache::new(),
            NotifyConfig::default(),
        );
        let mut debouncer = match debouncer {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[watch] could not start: {e}");
                return;
            }
        };

        // Flat storage, so a non-recursive watch on each folder is enough.
        for dir in [&keep_dir, &scratch_dir] {
            if let Err(e) = debouncer.watch(dir.as_path(), RecursiveMode::NonRecursive) {
                eprintln!("[watch] skipping {dir:?}: {e}");
            }
        }

        // Holding `debouncer` alive for the life of this loop is what keeps
        // the watch open; dropping it (by returning) stops the thread.
        loop {
            match rx.recv_timeout(GENERATION_POLL) {
                Ok(Ok(events)) => {
                    if events.iter().any(|e| !e.event.paths.is_empty()) {
                        rescan(&app);
                    }
                }
                Ok(Err(errors)) => {
                    for e in errors {
                        eprintln!("[watch] {e}");
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }

            if watch_generation.load(Ordering::SeqCst) != generation {
                return; // notesDir changed; a replacement watcher is running
            }
        }
    });
}

fn rescan(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.config.read().clone();
    let notes = scan::scan_all(&settings);
    *state.notes.write() = notes;
    let _ = app.emit(NOTES_CHANGED, ());
}
