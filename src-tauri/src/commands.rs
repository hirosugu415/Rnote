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
    let mut items = crate::notes::search::apply(&state.notes.read(), &filter);
    crate::notes::scan::refresh_expiry(&mut items, state.config.read().expiry_days);
    items
}

#[tauri::command]
pub fn list_tags(state: State<'_, AppState>) -> Vec<crate::notes::tags::TagCount> {
    crate::notes::tags::counts(&state.notes.read())
}

/// Rewrites `#from` to `#to` in every note that has it — a rename, or a
/// merge when `to` is already in use. Reads each file fresh from disk (not
/// the cache) so it can't resurrect stale content. Returns how many notes
/// changed.
#[tauri::command]
pub fn rename_tag(app: AppHandle, from: String, to: String) -> Result<usize, String> {
    use crate::notes::tags;
    let to = to.trim().trim_start_matches('#').to_string();
    if !tags::is_valid_name(&to) {
        return Err(format!("\"{to}\" はタグ名に使えません（文字・数字・_・- のみ）"));
    }
    if from == to {
        return Ok(0);
    }

    let state = app.state::<AppState>();
    let settings = state.config.read().clone();
    let targets: Vec<(String, Status)> = state
        .notes
        .read()
        .iter()
        .filter(|n| n.tags.contains(&from))
        .map(|n| (n.filename.clone(), n.status))
        .collect();

    let outcome = store::rename_tag(&settings, &targets, &from, &to);
    for (previous, note) in &outcome.updated {
        upsert(&state, previous, note);
    }
    let changed = outcome.updated.len();
    if outcome.failed.is_empty() {
        Ok(changed)
    } else {
        Err(format!(
            "{changed}件を変更、{}件失敗: {}",
            outcome.failed.len(),
            outcome.failed.join(", ")
        ))
    }
}

/// Error text for a note that exists neither in the cache nor on disk. The
/// frontend matches on it to recreate an open note deleted out from under it.
pub const NOTE_GONE: &str = "that note is gone";

/// Which folder `filename` lives in: the cache first, then disk — the cache
/// can briefly lag a file the watcher hasn't rescanned yet (or one a racing
/// rescan dropped), and that alone shouldn't fail a save.
fn locate(state: &AppState, filename: &str) -> Result<Status, String> {
    if let Some(n) = state.notes.read().iter().find(|n| n.filename == filename) {
        return Ok(n.status);
    }
    let settings = state.config.read();
    [Status::Keep, Status::Scratch]
        .into_iter()
        .find(|s| crate::notes::scan::status_dir(&settings, *s).join(filename).is_file())
        .ok_or_else(|| NOTE_GONE.into())
}

/// Puts `note` into the cache in place of whatever was cached under
/// `previous` (its name before a save/move renamed it) or under its own
/// name, then re-sorts — a save changes `modified_at`, i.e. its position.
fn upsert(state: &AppState, previous: &str, note: &Note) {
    let mut notes = state.notes.write();
    notes.retain(|n| n.filename != previous && n.filename != note.filename);
    notes.push(note.clone());
    crate::notes::sort_newest(&mut notes);
}

#[tauri::command]
pub fn get_note(state: State<'_, AppState>, filename: String) -> Result<Note, String> {
    // Read from disk, not the cache: the frontend compares this against what
    // it last saved to detect outside edits, and a watcher rescan racing one
    // of our own writes can leave the cache a version behind for a moment.
    let status = locate(&state, &filename)?;
    let settings = state.config.read().clone();
    let path = crate::notes::scan::status_dir(&settings, status).join(&filename);
    let note = crate::notes::scan::scan_one(&path, status, settings.expiry_days)
        .map_err(|_| NOTE_GONE.to_string())?;
    upsert(&state, &filename, &note);
    Ok(note)
}

#[tauri::command]
pub fn create_note(app: AppHandle, body: String, status: Status) -> Result<Note, String> {
    let state = app.state::<AppState>();
    let mut settings = state.config.read().clone();

    let note = store::create(&settings, &body, status).map_err(|e| e.to_string())?;

    {
        let mut notes = state.notes.write();
        notes.push(note.clone());
        crate::notes::sort_newest(&mut notes);
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
    let status = locate(&state, &filename)?;
    let settings = state.config.read().clone();
    let note = store::update(&settings, &filename, &body, status).map_err(|e| e.to_string())?;
    upsert(&state, &filename, &note);
    Ok(note)
}

#[tauri::command]
pub fn set_status(app: AppHandle, filename: String, status: Status) -> Result<Note, String> {
    let state = app.state::<AppState>();
    let from = locate(&state, &filename)?;
    let settings = state.config.read().clone();
    if from == status {
        let path = crate::notes::scan::status_dir(&settings, status).join(&filename);
        return crate::notes::scan::scan_one(&path, status, settings.expiry_days)
            .map_err(|e| e.to_string());
    }

    let note = store::set_status(&settings, &filename, from, status).map_err(|e| e.to_string())?;
    upsert(&state, &filename, &note);
    Ok(note)
}

#[tauri::command]
pub fn delete_note(app: AppHandle, filename: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let status = locate(&state, &filename)?;
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
pub fn set_settings(app: AppHandle, mut next: Settings) -> Result<Settings, String> {
    let state = app.state::<AppState>();
    let previous = state.config.read().clone();
    // Not edited in the settings panel, and the panel's copy is whatever it
    // was when the app started — keep the live value instead of rolling back
    // every Keep/Scratch choice made since.
    next.last_status = previous.last_status;
    next.zoom = previous.zoom; // same story: changed by Ctrl +/-, not the panel

    if previous.hotkey != next.hotkey {
        // Validate before storing, so a typo cannot leave the app with no
        // way to be summoned. `register_hotkey` drops the old binding before
        // trying the new one, so a key another app already holds would
        // otherwise leave nothing registered at all — put the old one back.
        if let Err(e) = crate::register_hotkey(&app, &next.hotkey) {
            let _ = crate::register_hotkey(&app, &previous.hotkey);
            return Err(e);
        }
    }

    if previous.autostart != next.autostart {
        crate::sync_autostart(&app, next.autostart)?;
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

/// Sets and remembers the UI zoom; returns the value actually applied
/// (clamped and rounded to whole percents).
#[tauri::command]
pub fn set_zoom(app: AppHandle, zoom: f64) -> Result<f64, String> {
    let applied = crate::apply_zoom(&app, zoom);
    let state = app.state::<AppState>();
    let mut settings = state.config.read().clone();
    if settings.zoom != applied {
        settings.zoom = applied;
        config::save(&settings).map_err(|e| e.to_string())?;
        *state.config.write() = settings;
    }
    Ok(applied)
}

#[tauri::command]
pub fn hide_window(app: AppHandle) {
    crate::hide_window(&app);
}
