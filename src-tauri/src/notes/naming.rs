//! Filename generation: the note's title (its first line, heading marker
//! stripped) as a Windows-safe name — `買い物.md`. Re-derived on every save
//! (see `store::update`), so the file in Explorer always reads like the note.
//!
//! Older notes were named `YYYY-MM-DD_HHmm - slug.md`; `scan.rs` still parses
//! that prefix for `created_at`, and such a file is renamed to the new scheme
//! the next time it is saved.

use std::path::Path;

/// Characters short enough to keep long first lines from making unwieldy
/// paths, long enough to tell notes apart at a glance.
const NAME_MAX: usize = 60;
const ILLEGAL: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
/// Device names Windows refuses as a file stem, whatever the extension.
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];
/// Stem used when the body has no title line yet.
const UNTITLED: &str = "無題";

/// `strftime` pattern of the legacy timestamp prefix — no longer written,
/// only parsed back by `scan.rs` for notes created before the rename.
pub(crate) const TIMESTAMP_FORMAT: &str = "%Y-%m-%d_%H%M";

/// The filename `body` should have: `<title>.md`, or `<title> (2).md`… when
/// that name is taken in any of `dirs`. Names are kept unique across keep/
/// and scratch/ together, because the app identifies a note by filename
/// alone. `current` is the note's existing name, if any — it never counts as
/// a collision with itself (so an unchanged title keeps its name, and a
/// case-only change like `abc` → `ABC` doesn't pick up a suffix on
/// case-insensitive NTFS).
pub fn generate(body: &str, dirs: &[&Path], current: Option<&str>) -> String {
    let stem = stem_of(body);
    let is_self = |name: &str| current.is_some_and(|c| c.eq_ignore_ascii_case(name));
    let free = |name: &str| is_self(name) || !dirs.iter().any(|d| d.join(name).exists());

    let candidate = format!("{stem}.md");
    if free(&candidate) {
        return candidate;
    }
    for n in 2.. {
        let candidate = format!("{stem} ({n}).md");
        if free(&candidate) {
            return candidate;
        }
    }
    unreachable!("filename suffix search is unbounded")
}

fn stem_of(body: &str) -> String {
    let line = super::title_line(body).map_or("", |(_, l)| l);
    let cleaned: String = line
        .chars()
        .map(|c| if ILLEGAL.contains(&c) || c.is_control() { ' ' } else { c })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let capped: String = collapsed.chars().take(NAME_MAX).collect();
    // Windows silently drops trailing dots/spaces, which would make the name
    // we return differ from the one actually on disk.
    let trimmed = capped.trim_end_matches(['.', ' ']);
    if trimmed.is_empty() {
        UNTITLED.to_string()
    } else if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(trimmed)) {
        format!("{trimmed}_")
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rnote-naming-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn name_is_the_title_without_illegal_characters() {
        let dir = test_dir("illegal");
        let name = generate("con:tent? \"quote\" <tag> | pipe\nbody", &[&dir], None);
        assert_eq!(name, "con tent quote tag pipe.md");
    }

    #[test]
    fn heading_marker_trailing_dots_and_reserved_names() {
        let dir = test_dir("heading");
        assert_eq!(generate("# 買い物\n牛乳", &[&dir], None), "買い物.md");
        assert_eq!(generate("end...", &[&dir], None), "end.md");
        assert_eq!(generate("con", &[&dir], None), "con_.md");
        assert_eq!(generate("\n\n", &[&dir], None), "無題.md");
    }

    #[test]
    fn collisions_are_checked_across_every_dir_but_not_against_itself() {
        let keep = test_dir("collide-keep");
        let scratch = test_dir("collide-scratch");
        std::fs::write(keep.join("memo.md"), "x").unwrap();
        assert_eq!(generate("memo", &[&keep, &scratch], None), "memo (2).md");
        assert_eq!(generate("memo", &[&keep, &scratch], Some("memo.md")), "memo.md");
        assert_eq!(generate("MEMO", &[&keep, &scratch], Some("memo.md")), "MEMO.md");
    }
}
