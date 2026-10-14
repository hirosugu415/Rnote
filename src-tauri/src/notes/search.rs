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
    // Whitespace-separated terms, all of which must match (AND) — so
    // `会議 #仕事` finds notes containing both, not only that exact phrase.
    let terms: Vec<String> = filter
        .query
        .as_deref()
        .unwrap_or("")
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();

    notes
        .iter()
        .filter(|n| filter.status.is_none_or(|s| s == n.status))
        .filter(|n| {
            if terms.is_empty() {
                return true;
            }
            let title = n.title.to_lowercase();
            let body = n.body.to_lowercase();
            terms.iter().all(|q| {
                // `#仕事` should also hit the tag `仕事` (stored without `#`).
                let bare = q.strip_prefix('#').filter(|b| !b.is_empty()).unwrap_or(q);
                title.contains(q.as_str())
                    || body.contains(q.as_str())
                    || n.tags.iter().any(|t| t.to_lowercase().contains(bare))
            })
        })
        .map(|n| {
            let mut summary = n.summary();
            if let Some(snippet) = super::snippet_around(&n.body, &terms) {
                summary.snippet = snippet;
            }
            summary
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(body: &str) -> Note {
        Note {
            filename: body.into(),
            status: Status::Keep,
            title: body.lines().next().unwrap_or("").into(),
            body: body.into(),
            tags: crate::notes::tags::extract(body),
            created_at: String::new(),
            modified_at: String::new(),
            is_expiring: false,
            char_count: 0,
        }
    }

    #[test]
    fn every_term_must_match() {
        let notes = vec![note("会議メモ #仕事"), note("会議の議事録"), note("買い物 #仕事")];
        let filter = Filter { status: None, query: Some("会議  #仕事".into()) };
        let hits: Vec<_> = apply(&notes, &filter).into_iter().map(|n| n.filename).collect();
        assert_eq!(hits, vec!["会議メモ #仕事"]);
    }
}
