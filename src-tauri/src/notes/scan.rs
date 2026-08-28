//! Scans `keep/` and `scratch/` into `Note`s. No recursion — storage is
//! deliberately flat, so this is one `read_dir` per status.

use super::naming::{SEPARATOR, TIMESTAMP_FORMAT};
use super::{tags, Note};
use crate::config::{Settings, Status};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use std::path::{Path, PathBuf};

/// Byte length of a formatted `TIMESTAMP_FORMAT` stamp — `"2026-08-26_1430"`,
/// always ASCII so bytes and chars agree.
const STAMP_LEN: usize = 15;

/// Parses the `YYYY-MM-DD_HHmm` prefix Rnote itself writes. Anything that
/// doesn't match — a file the user dropped in or renamed by hand — falls
/// through to filesystem metadata in `scan_one`, which is the whole point of
/// keeping notes as plain, ordinary `.md` files.
fn parse_filename_timestamp(filename: &str) -> Option<DateTime<Local>> {
    let stem = filename.strip_suffix(".md")?;
    let stamp = stem.get(0..STAMP_LEN)?;
    let naive = NaiveDateTime::parse_from_str(stamp, TIMESTAMP_FORMAT).ok()?;
    Local.from_local_datetime(&naive).single()
}

fn title_of(filename: &str, body: &str) -> String {
    // The slug half of our own naming convention, if there is one, past the
    // timestamp and its separator.
    if let Some(stem) = filename.strip_suffix(".md") {
        if let Some(rest) = stem.get(STAMP_LEN + SEPARATOR.len()..) {
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    body.lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "(無題)".to_string())
}

pub(crate) fn scan_one(path: &Path, status: Status, expiry_days: u32) -> std::io::Result<Note> {
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    let body = std::fs::read_to_string(path)?;
    let meta = std::fs::metadata(path)?;

    // created_at: filename timestamp first (survives copies/backups, unlike
    // NTFS creation time — see the module doc), then fs created(), then
    // fs modified(), so this never has nothing to fall back on.
    let created_at = parse_filename_timestamp(&filename)
        .or_else(|| meta.created().ok().map(DateTime::<Local>::from))
        .or_else(|| meta.modified().ok().map(DateTime::<Local>::from))
        .unwrap_or_else(Local::now);
    let modified_at = meta.modified().ok().map(DateTime::<Local>::from).unwrap_or(created_at);

    let tags = tags::extract(&body);
    let is_expiring = status == Status::Scratch
        && Local::now().signed_duration_since(created_at).num_days() >= expiry_days as i64;

    Ok(Note {
        title: title_of(&filename, &body),
        char_count: body.chars().count(),
        filename,
        status,
        body,
        tags,
        created_at: created_at.to_rfc3339(),
        modified_at: modified_at.to_rfc3339(),
        is_expiring,
    })
}

fn scan_dir(dir: &Path, status: Status, expiry_days: u32) -> Vec<Note> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("md"))
        .filter_map(|e| match scan_one(&e.path(), status, expiry_days) {
            Ok(note) => Some(note),
            Err(err) => {
                eprintln!("[scan] skipping {:?}: {err}", e.path());
                None
            }
        })
        .collect()
}

pub fn status_dir(settings: &Settings, status: Status) -> PathBuf {
    settings.notes_dir.join(status.dirname())
}

/// Full scan of both folders, newest first. Creates the folders on a first
/// run (or after the notes directory setting changes).
pub fn scan_all(settings: &Settings) -> Vec<Note> {
    let keep_dir = status_dir(settings, Status::Keep);
    let scratch_dir = status_dir(settings, Status::Scratch);
    let _ = std::fs::create_dir_all(&keep_dir);
    let _ = std::fs::create_dir_all(&scratch_dir);

    let mut notes = scan_dir(&keep_dir, Status::Keep, settings.expiry_days);
    notes.extend(scan_dir(&scratch_dir, Status::Scratch, settings.expiry_days));
    notes.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    notes
}
