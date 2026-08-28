//! Filename generation: a timestamp plus a slug from the first line, decided
//! once at creation and never revisited afterwards — Explorer's sort order,
//! and the `created_at` scan.rs derives from this same prefix, both depend
//! on the name staying stable even if the note's content changes later.

use chrono::{DateTime, Local};
use std::path::Path;

const SLUG_MAX: usize = 40;
const ILLEGAL: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

/// Separates the timestamp from the slug. Kept as a named constant since
/// `scan.rs` has to parse back past exactly this many bytes.
pub(crate) const SEPARATOR: &str = " - ";
/// `strftime` pattern for the timestamp half of the name — minute
/// resolution is plenty for something a human reads in Explorer, and
/// `scan.rs` mirrors this exact pattern when parsing it back.
pub(crate) const TIMESTAMP_FORMAT: &str = "%Y-%m-%d_%H%M";

/// Builds e.g. `2026-08-26_1430 - buy milk.md`, avoiding a collision with
/// anything already in `dir` — two captures inside the same minute are
/// common at this resolution, not an edge case.
pub fn generate(body: &str, when: DateTime<Local>, dir: &Path) -> String {
    let stamp = when.format(TIMESTAMP_FORMAT).to_string();
    let slug = slugify(first_line(body));
    let base = if slug.is_empty() { stamp } else { format!("{stamp}{SEPARATOR}{slug}") };

    let candidate = format!("{base}.md");
    if !dir.join(&candidate).exists() {
        return candidate;
    }
    for n in 2.. {
        let candidate = format!("{base}_{n}.md");
        if !dir.join(&candidate).exists() {
            return candidate;
        }
    }
    unreachable!("filename suffix search is unbounded")
}

fn first_line(body: &str) -> &str {
    body.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim()
}

fn slugify(line: &str) -> String {
    let cleaned: String = line
        .chars()
        .map(|c| if ILLEGAL.contains(&c) || c.is_control() { ' ' } else { c })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(SLUG_MAX).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn scratch_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rnote-naming-test-{:?}", std::thread::current().id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn strips_illegal_characters_and_caps_length() {
        let when = Local.with_ymd_and_hms(2026, 8, 26, 14, 30, 12).unwrap();
        let dir = scratch_dir();
        let name = generate("con:tent? \"quote\" <tag> | pipe", when, &dir);
        assert!(!name.contains([':', '?', '"', '<', '>', '|']));
        assert!(name.starts_with("2026-08-26_1430 - "));
    }

    #[test]
    fn avoids_same_minute_collisions() {
        let when = Local.with_ymd_and_hms(2026, 8, 26, 14, 30, 12).unwrap();
        let dir = scratch_dir();
        let first = generate("same body", when, &dir);
        std::fs::write(dir.join(&first), "x").unwrap();
        let second = generate("same body", when, &dir);
        assert_ne!(first, second);
    }
}
