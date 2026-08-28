//! The IPC surface — thin wrappers over `notes::*` and `config`.

use crate::config::{self, Settings, Status};
use crate::notes::{search::Filter, store, Note, NoteSummary};
use crate::state::AppState;
use crate::watcher;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub fn list_notes(state: State<'_, AppState>, filter: Filter) -> Vec<NoteSummary> {
    crate::notes::search::apply(&state.notes.read(), &filter)
}

#[tauri::command]
pub fn get_note(state: State<'_, AppState>, filename: String) -> Result<Note, String> {
    find(&state, &filename)
}

fn find(state: &State<'_, AppState>, filename: &str) -> Result<Note, String> {
    state
        .notes
        .read()
        .iter()
        .find(|n| n.filename == filename)
        .cloned()
        .ok_or_else(|| "that note is gone".into())
}

#[tauri::command]
pub fn create_note(app: AppHandle, body: String, status: Status) -> Result<Note, String> {
    let state = app.state::<AppState>();
    let mut settings = state.config.read().clone();

    let note = store::create(&settings, &body, status).map_err(|e| e.to_string())?;

    {
        let mut notes = state.notes.write();
        notes.push(note.clone());
        notes.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    }

    if settings.last_status != status {
        settings.last_status = status;
        config::save(&settings).map_err(|e| e.to_string())?;
        *state.config.write() = settings;
    }

    Ok(note)
}

#[tauri::command]
pub fn update_note(app: AppHandle, filename: String, body: String) -> Result<Note, String> {
    let state = app.state::<AppState>();
    let status = find(&state, &filename)?.status;
    let settings = state.config.read().clone();
    let note = store::update(&settings, &filename, &body, status).map_err(|e| e.to_string())?;

    let mut notes = state.notes.write();
    if let Some(slot) = notes.iter_mut().find(|n| n.filename == filename) {
        *slot = note.clone();
    }
    Ok(note)
}

#[tauri::command]
pub fn set_status(app: AppHandle, filename: String, status: Status) -> Result<Note, String> {
    let state = app.state::<AppState>();
    let from = find(&state, &filename)?.status;
    if from == status {
        return find(&state, &filename);
    }

    let settings = state.config.read().clone();
    let note = store::set_status(&settings, &filename, from, status).map_err(|e| e.to_string())?;

    let mut notes = state.notes.write();
    if let Some(slot) = notes.iter_mut().find(|n| n.filename == filename) {
        *slot = note.clone();
    }
    Ok(note)
}

#[tauri::command]
pub fn delete_note(app: AppHandle, filename: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let status = find(&state, &filename)?.status;
    let settings = state.config.read().clone();
    store::delete(&settings, &filename, status).map_err(|e| e.to_string())?;
    state.notes.write().retain(|n| n.filename != filename);
    Ok(())
}

/// Opens Explorer at the notes root — the parent of both `keep/` and
/// `scratch/` — since the value here is browsing both at once.
#[tauri::command]
pub fn open_notes_folder(app: AppHandle) -> Result<(), String> {
    let notes_dir = app.state::<AppState>().config.read().notes_dir.clone();
    std::fs::create_dir_all(&notes_dir).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(notes_dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.config.read().clone()
}

#[tauri::command]
pub fn set_settings(app: AppHandle, next: Settings) -> Result<Settings, String> {
    let state = app.state::<AppState>();
    let previous = state.config.read().clone();

    if previous.hotkey != next.hotkey {
        // Validate before storing, so a typo cannot leave the app with no
        // way to be summoned.
        crate::register_hotkey(&app, &next.hotkey)?;
    }

    config::save(&next).map_err(|e| e.to_string())?;
    *state.config.write() = next.clone();

    if previous.notes_dir != next.notes_dir {
        // Deliberately does not migrate old notes from the previous
        // location — out of scope for v1, the UI surfaces this as a warning.
        std::fs::create_dir_all(next.notes_dir.join(Status::Keep.dirname()))
            .map_err(|e| e.to_string())?;
        std::fs::create_dir_all(next.notes_dir.join(Status::Scratch.dirname()))
            .map_err(|e| e.to_string())?;
        *state.notes.write() = crate::notes::scan::scan_all(&next);
        state.watch_generation.fetch_add(1, Ordering::SeqCst);
        watcher::spawn(app.clone());
    } else if previous.expiry_days != next.expiry_days {
        *state.notes.write() = crate::notes::scan::scan_all(&next);
    }

    Ok(next)
}

#[tauri::command]
pub fn hide_window(app: AppHandle) {
    crate::hide_window(&app);
}
