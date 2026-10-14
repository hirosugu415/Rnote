//! The write side: everything that actually touches `keep/`/`scratch/` on
//! disk. Every call here is a direct, synchronous `fs` operation — a note is
//! one small text file, and there is no batching to be gained by deferring
//! it (contrast Rclip's autosaved history blob).

use super::scan;
use crate::config::{Settings, Status};
use std::path::{Path, PathBuf};

fn both_dirs(settings: &Settings) -> [PathBuf; 2] {
    [scan::status_dir(settings, Status::Keep), scan::status_dir(settings, Status::Scratch)]
}

/// The name `body` should be saved under, unique across keep/ and scratch/.
fn name_for(settings: &Settings, body: &str, current: Option<&str>) -> String {
    let [keep, scratch] = both_dirs(settings);
    super::naming::generate(body, &[keep.as_path(), scratch.as_path()], current)
}

pub fn create(settings: &Settings, body: &str, status: Status) -> std::io::Result<super::Note> {
    let dir = scan::status_dir(settings, status);
    std::fs::create_dir_all(&dir)?;
    let filename = name_for(settings, body, None);
    std::fs::write(dir.join(&filename), body)?;
    scan::scan_one(&dir.join(&filename), status, settings.expiry_days)
}

/// Overwrites the file, then renames it if its title (first line) changed —
/// the filename always follows the title. A failed rename is not a failed
/// save: the content is already written, so it just keeps the old name.
pub fn update(
    settings: &Settings,
    filename: &str,
    body: &str,
    status: Status,
) -> std::io::Result<super::Note> {
    let dir = scan::status_dir(settings, status);
    let path = dir.join(filename);
    std::fs::write(&path, body)?;

    let wanted = name_for(settings, body, Some(filename));
    let final_path = if wanted != filename {
        let to = dir.join(&wanted);
        match rename_no_clobber(&path, &to) {
            Ok(()) => to,
            Err(e) => {
                eprintln!("[store] could not rename {filename:?} -> {wanted:?}: {e}");
                path
            }
        }
    } else {
        path
    };
    scan::scan_one(&final_path, status, settings.expiry_days)
}

/// Moves the file between `keep/` and `scratch/`, re-picking its name so it
/// cannot land on (and, with `fs::rename`'s replace semantics on Windows,
/// silently overwrite) a same-named note already in the other folder.
pub fn set_status(
    settings: &Settings,
    filename: &str,
    from: Status,
    to: Status,
) -> std::io::Result<super::Note> {
    let to_dir = scan::status_dir(settings, to);
    std::fs::create_dir_all(&to_dir)?;
    let from_path = scan::status_dir(settings, from).join(filename);
    let body = std::fs::read_to_string(&from_path)?;
    let to_path = to_dir.join(name_for(settings, &body, Some(filename)));
    rename_no_clobber(&from_path, &to_path)?;
    scan::scan_one(&to_path, to, settings.expiry_days)
}

pub struct RenameTagOutcome {
    /// (filename before the save, the note as saved — possibly renamed).
    pub updated: Vec<(String, super::Note)>,
    pub failed: Vec<String>,
}

/// Rewrites `#from` to `#to` in each of `targets`, read fresh from disk.
pub fn rename_tag(
    settings: &Settings,
    targets: &[(String, Status)],
    from: &str,
    to: &str,
) -> RenameTagOutcome {
    let mut outcome = RenameTagOutcome { updated: Vec::new(), failed: Vec::new() };
    for (filename, status) in targets {
        let path = scan::status_dir(settings, *status).join(filename);
        let result = std::fs::read_to_string(&path).and_then(|body| {
            match super::tags::rename_in_body(&body, from, to) {
                Some(next) => update(settings, filename, &next, *status).map(Some),
                None => Ok(None),
            }
        });
        match result {
            Ok(Some(note)) => outcome.updated.push((filename.clone(), note)),
            Ok(None) => {}
            Err(e) => outcome.failed.push(format!("{filename}: {e}")),
        }
    }
    outcome
}

/// `fs::rename`, but refusing to replace a *different* file at `to`. A
/// case-only rename (`abc.md` → `ABC.md`) is the same file on NTFS and allowed.
fn rename_no_clobber(from: &Path, to: &Path) -> std::io::Result<()> {
    let same_name = from
        .file_name()
        .zip(to.file_name())
        .is_some_and(|(a, b)| a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy()))
        && from.parent() == to.parent();
    if to.exists() && !same_name {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("{} already exists", to.display()),
        ));
    }
    std::fs::rename(from, to)
}

/// Sends the note to the Windows Recycle Bin, so a mistaken delete can be
/// restored from there.
pub fn delete(settings: &Settings, filename: &str, status: Status) -> std::io::Result<()> {
    let path = scan::status_dir(settings, status).join(filename);
    if !path.exists() {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"));
    }
    move_to_recycle_bin(&path)
}

#[cfg(windows)]
fn move_to_recycle_bin(path: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{
        SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE,
        SHFILEOPSTRUCTW,
    };

    // The shell wants an absolute path in a list terminated by two NULs.
    let absolute = std::path::absolute(path)?;
    let mut from: Vec<u16> = absolute.as_os_str().encode_wide().collect();
    from.extend([0, 0]);

    let mut op = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        pTo: std::ptr::null(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };
    // SAFETY: `op` is fully initialised and `from` outlives the call.
    let code = unsafe { SHFileOperationW(&mut op) };
    if code != 0 || op.fAnyOperationsAborted != 0 {
        return Err(std::io::Error::other(format!(
            "could not move to the Recycle Bin (code {code})"
        )));
    }
    Ok(())
}

#[cfg(not(windows))]
fn move_to_recycle_bin(path: &Path) -> std::io::Result<()> {
    std::fs::remove_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(name: &str) -> Settings {
        let dir = std::env::temp_dir().join(format!("rnote-store-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        Settings { notes_dir: dir, ..Settings::default() }
    }

    #[test]
    fn saving_a_new_title_renames_the_file() {
        let s = settings("rename");
        let note = create(&s, "first title\nbody", Status::Scratch).unwrap();
        assert_eq!(note.filename, "first title.md");
        let note = update(&s, &note.filename, "second title\nbody", Status::Scratch).unwrap();
        assert_eq!(note.filename, "second title.md");
        assert!(!scan::status_dir(&s, Status::Scratch).join("first title.md").exists());
    }

    #[test]
    fn renaming_a_tag_onto_an_existing_one_merges_them() {
        let s = settings("merge-tag");
        let a = create(&s, "#自己分析 今日のふりかえり
本文", Status::Keep).unwrap();
        let b = create(&s, "別のメモ
#自己分析 と #振り返り", Status::Scratch).unwrap();
        let c = create(&s, "既存
#振り返り", Status::Keep).unwrap();
        let targets = vec![
            (a.filename.clone(), Status::Keep),
            (b.filename.clone(), Status::Scratch),
        ];
        let out = rename_tag(&s, &targets, "自己分析", "振り返り");
        assert!(out.failed.is_empty(), "{:?}", out.failed);
        assert_eq!(out.updated.len(), 2);
        for (_, note) in &out.updated {
            assert!(note.tags.contains(&"振り返り".to_string()), "{:?}", note.tags);
            assert!(!note.tags.contains(&"自己分析".to_string()));
        }
        // The note whose title carried the tag was renamed to match.
        assert_eq!(out.updated[0].1.filename, "#振り返り 今日のふりかえり.md");
        assert_eq!(c.tags, vec!["振り返り"]);
    }

    #[test]
    fn moving_never_overwrites_a_same_named_note() {
        let s = settings("move");
        let kept = create(&s, "memo", Status::Keep).unwrap();
        let scratch = create(&s, "memo", Status::Scratch).unwrap();
        assert_eq!((kept.filename.as_str(), scratch.filename.as_str()), ("memo.md", "memo (2).md"));
        let moved = set_status(&s, &scratch.filename, Status::Scratch, Status::Keep).unwrap();
        assert_eq!(moved.filename, "memo (2).md");
        let keep_dir = scan::status_dir(&s, Status::Keep);
        assert_eq!(std::fs::read_to_string(keep_dir.join("memo.md")).unwrap(), "memo");
    }
}

#[cfg(all(test, windows))]
mod recycle_tests {
    /// Touches the real Recycle Bin, so opt-in: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn delete_moves_the_file_to_the_recycle_bin() {
        let dir = std::env::temp_dir().join("rnote-recycle-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rnote-recycle-test.md");
        std::fs::write(&path, "x").unwrap();
        super::move_to_recycle_bin(&path).unwrap();
        assert!(!path.exists());
    }
}
