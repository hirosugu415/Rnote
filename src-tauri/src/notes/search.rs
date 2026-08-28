//! Pure in-memory filtering over the note cache — no disk IO, so it is cheap
//! enough to run on every keystroke with no debounce needed on this side.

use super::{Note, NoteSummary};
use crate::config::Status;
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Filter {
    pub status: Option<Status>,
    pub query: Option<String>,
}

pub fn apply(notes: &[Note], filter: &Filter) -> Vec<NoteSummary> {
    let query = filter
        .query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(str::to_lowercase);

    notes
        .iter()
        .filter(|n| filter.status.is_none_or(|s| s == n.status))
        .filter(|n| match &query {
            None => true,
            Some(q) => {
                n.title.to_lowercase().contains(q)
                    || n.body.to_lowercase().contains(q)
                    || n.tags.iter().any(|t| t.to_lowercase().contains(q))
            }
        })
        .map(Note::summary)
        .collect()
}
