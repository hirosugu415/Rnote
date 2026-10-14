//! The note domain: a `Note` is nothing but a `.md` file under `keep/` or
//! `scratch/`, plus everything derivable from its filename, content, and
//! filesystem metadata. There is no database and no sidecar file — the
//! folder on disk *is* the data, which is what keeps it browsable in
//! Explorer with the app not even running.

pub mod naming;
pub mod scan;
pub mod search;
pub mod store;
pub mod tags;

use crate::config::Status;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// The on-disk filename, also its stable identity — there is no
    /// separate id, since the filename already is one.
    pub filename: String,
    /// Derived purely from which folder this was scanned out of; never
    /// written into the file itself.
    pub status: Status,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub created_at: String,
    pub modified_at: String,
    pub is_expiring: bool,
    pub char_count: usize,
}

/// What the list view gets: everything but the full body, so a long note
/// doesn't cost its whole content on every `list_notes` round-trip.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSummary {
    pub filename: String,
    pub status: Status,
    pub title: String,
    pub snippet: String,
    pub tags: Vec<String>,
    pub created_at: String,
    pub modified_at: String,
    pub is_expiring: bool,
    pub char_count: usize,
}

impl Note {
    pub fn summary(&self) -> NoteSummary {
        NoteSummary {
            filename: self.filename.clone(),
            status: self.status,
            title: self.title.clone(),
            snippet: snippet_of(&self.body),
            tags: self.tags.clone(),
            created_at: self.created_at.clone(),
            modified_at: self.modified_at.clone(),
            is_expiring: self.is_expiring,
            char_count: self.char_count,
        }
    }
}

/// List order everywhere: most recently modified first, so a note you just
/// edited comes back to the top no matter how old it is.
pub fn sort_newest(notes: &mut [Note]) {
    let key = |n: &Note| chrono::DateTime::parse_from_rfc3339(&n.modified_at).ok();
    notes.sort_by(|a, b| key(b).cmp(&key(a)));
}

const SNIPPET_LIMIT: usize = 200;

/// A line's text with any Markdown heading marker (`# `, `## `…) removed, so
/// `# 買い物` titles as `買い物`. A bare `#` (nothing typed after it yet)
/// becomes empty; `#tag` with no space is a tag, not a heading, and is kept.
pub(crate) fn strip_heading(line: &str) -> &str {
    let line = line.trim();
    let rest = line.trim_start_matches('#');
    if rest.len() == line.len() {
        return line;
    }
    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
        rest.trim()
    } else {
        line
    }
}

/// Index and text of the line that serves as the note's title: the first
/// line that is non-empty once its heading marker is stripped. Shared by the
/// displayed title, the filename slug, and the snippet, so all three agree.
pub(crate) fn title_line(body: &str) -> Option<(usize, &str)> {
    body.lines()
        .enumerate()
        .map(|(i, l)| (i, strip_heading(l)))
        .find(|(_, l)| !l.is_empty())
}

/// Whitespace-collapsed first ~200 characters of the body *after* its title
/// line. That line is shown on its own in the list row, so repeating it here
/// would just waste the row's two snippet lines. A note that is nothing but a
/// title yields an empty snippet.
fn snippet_of(body: &str) -> String {
    let skip = title_line(body).map_or(0, |(i, _)| i + 1);
    let rest = body.lines().skip(skip).collect::<Vec<_>>().join("\n");
    let collapsed = rest.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= SNIPPET_LIMIT {
        collapsed
    } else {
        collapsed.chars().take(SNIPPET_LIMIT).collect::<String>() + "…"
    }
}

/// Characters of context kept before the first match in a search snippet.
const SNIPPET_LEAD: usize = 30;

/// Per-character lowercase used for matching. Comparing char-by-char (rather
/// than lowercasing whole strings) keeps match positions valid in the
/// original text even for the rare letters whose lowercase is longer.
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// A search-result snippet centred on the first place in the body (after
/// the title line) where any of `terms` occurs — `…前後の文脈 match 続き…` —
/// so the list shows *why* a note matched instead of just its opening. A
/// leading `#` on a term is ignored, so `#仕事` finds `仕事`. `None` when no
/// term occurs outside the title (the regular opening snippet is used then).
pub fn snippet_around(body: &str, terms: &[String]) -> Option<String> {
    let skip = title_line(body).map_or(0, |(i, _)| i + 1);
    let rest = body.lines().skip(skip).collect::<Vec<_>>().join("\n");
    let chars: Vec<char> = rest.split_whitespace().collect::<Vec<_>>().join(" ").chars().collect();
    let folded: Vec<char> = chars.iter().map(|&c| fold(c)).collect();

    let needles: Vec<Vec<char>> = terms
        .iter()
        .map(|t| t.strip_prefix('#').filter(|b| !b.is_empty()).unwrap_or(t))
        .map(|t| t.chars().map(fold).collect::<Vec<_>>())
        .filter(|n| !n.is_empty())
        .collect();
    let first = needles
        .iter()
        .filter_map(|n| folded.windows(n.len()).position(|w| w == n.as_slice()))
        .min()?;

    let start = first.saturating_sub(SNIPPET_LEAD);
    let end = (start + SNIPPET_LIMIT).min(chars.len());
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(&chars[start..end]);
    if end < chars.len() {
        out.push('…');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_is_centred_on_the_first_match() {
        let filler = "あ".repeat(100);
        let body = format!("タイトル\n{filler}ここに Rust の話\n続き");
        let s = snippet_around(&body, &["rust".into()]).unwrap();
        assert!(s.starts_with('…'), "{s}");
        assert!(s.contains("ここに Rust の話 続き"), "{s}");
        // Only in the title: no special snippet.
        assert_eq!(snippet_around("Rust だけ\n本文", &["rust".into()]), None);
        // `#tag` terms match the bare word.
        assert!(snippet_around("t\n#仕事 です", &["#仕事".into()]).is_some());
    }

    #[test]
    fn heading_markers_are_stripped_but_tags_are_not() {
        assert_eq!(strip_heading("# 買い物"), "買い物");
        assert_eq!(strip_heading("### deep "), "deep");
        assert_eq!(strip_heading("#"), "");
        assert_eq!(strip_heading("#tag line"), "#tag line");
    }

    #[test]
    fn title_skips_a_bare_hash_line_and_snippet_follows_it() {
        let body = "#\n\n# Title\nbody text";
        assert_eq!(title_line(body), Some((2, "Title")));
        assert_eq!(snippet_of(body), "body text");
    }
}
