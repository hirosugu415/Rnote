//! The write side: everything that actually touches `keep/`/`scratch/` on
//! disk. Every call here is a direct, synchronous `fs` operation — a note is
//! one small text file, and there is no batching to be gained by deferring
//! it (contrast Rclip's autosaved history blob).

use super::scan;
use crate::config::{Settings, Status};
use chrono::Local;

pub fn create(settings: &Settings, body: &str, status: Status) -> std::io::Result<super::Note> {
    let dir = scan::status_dir(settings, status);
    std::fs::create_dir_all(&dir)?;
    let filename = super::naming::generate(body, Local::now(), &dir);
    std::fs::write(dir.join(&filename), body)?;
    scan::scan_one(&dir.join(&filename), status, settings.expiry_days)
}

/// Overwrites the file in place. The *filename* never changes, even if the
/// first line changes with it — only the slug a brand-new note would have
/// used is frozen. The displayed title is re-derived from the first line on
/// every scan (see `scan::title_of`), so it does follow the edit.
pub fn update(
    settings: &Settings,
    filename: &str,
    body: &str,
    status: Status,
) -> std::io::Result<super::Note> {
    let path = scan::status_dir(settings, status).join(filename);
    std::fs::write(&path, body)?;
    scan::scan_one(&path, status, settings.expiry_days)
}

/// Moves the file between `keep/` and `scratch/`. Same volume, so this is a
/// metadata-only rename.
pub fn set_status(
    settings: &Settings,
    filename: &str,
    from: Status,
    to: Status,
) -> std::io::Result<super::Note> {
    let to_dir = scan::status_dir(settings, to);
    std::fs::create_dir_all(&to_dir)?;
    let from_path = scan::status_dir(settings, from).join(filename);
    let to_path = to_dir.join(filename);
    std::fs::rename(&from_path, &to_path)?;
    scan::scan_one(&to_path, to, settings.expiry_days)
}

/// Explicit only — there is no soft-delete or trash for v1. Explorer's own
/// recycle bin still applies to anyone who deletes the file from there
/// instead of through the app.
pub fn delete(settings: &Settings, filename: &str, status: Status) -> std::io::Result<()> {
    std::fs::remove_file(scan::status_dir(settings, status).join(filename))
}
