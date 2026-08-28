//! Shared application state: the settings and the in-memory note cache every
//! command reads and writes.

use crate::config::Settings;
use crate::notes::Note;
use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize};
use std::sync::Arc;

pub struct AppState {
    pub config: Arc<RwLock<Settings>>,
    /// The whole note set, loaded once at startup and kept current by
    /// optimistic updates from every mutating command plus a filesystem
    /// watcher as a safety net. See `notes::search` for why re-scanning disk
    /// per query would be the wrong call at this scale.
    pub notes: Arc<RwLock<Vec<Note>>>,
    /// Bumped whenever the watched folders change (a `notesDir` setting
    /// update), so a stale watcher thread knows to let go of its watch and
    /// exit rather than race the replacement.
    pub watch_generation: Arc<AtomicUsize>,
    /// Tracked by polling the OS rather than from `WindowEvent::Focused`,
    /// because WebView2 keeps keyboard focus in a child HWND — see
    /// `spawn_focus_watch` in `lib.rs`.
    pub focused: Arc<AtomicBool>,
    /// Our own window, so the focus-watch thread can tell whether the
    /// foreground window is us.
    pub our_hwnd: Arc<AtomicIsize>,
}

impl AppState {
    pub fn new(config: Settings, notes: Vec<Note>) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            notes: Arc::new(RwLock::new(notes)),
            watch_generation: Arc::new(AtomicUsize::new(0)),
            focused: Arc::new(AtomicBool::new(true)),
            our_hwnd: Arc::new(AtomicIsize::new(0)),
        }
    }
}
