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

const SNIPPET_LIMIT: usize = 200;

/// Whitespace-collapsed first ~200 characters, for the list row.
fn snippet_of(body: &str) -> String {
    let collapsed = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= SNIPPET_LIMIT {
        collapsed
    } else {
        collapsed.chars().take(SNIPPET_LIMIT).collect::<String>() + "…"
    }
}
