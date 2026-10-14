//! Inline `#tag` extraction. Hand-rolled rather than pulling in `regex` —
//! matches the house preference (see Rclip's memchr-based search) for a scan
//! this simple not to need a dependency, and stays Unicode-aware so
//! `#仕事`-style tags work.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TagCount {
    pub tag: String,
    /// Number of notes carrying the tag (each note counts once).
    pub count: usize,
}

/// Every tag across `notes` with how many notes use it — most used first,
/// ties by name. Feeds the search screen's tag browser and the compose
/// box's `#` completion.
pub fn counts(notes: &[super::Note]) -> Vec<TagCount> {
    let mut map = std::collections::HashMap::<&str, usize>::new();
    for note in notes {
        for tag in &note.tags {
            *map.entry(tag.as_str()).or_default() += 1;
        }
    }
    let mut out: Vec<TagCount> = map
        .into_iter()
        .map(|(tag, count)| TagCount { tag: tag.to_string(), count })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.tag.cmp(&b.tag)));
    out
}

/// Tags in first-appearance order, deduplicated, without the leading `#`.
pub fn extract(body: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '#' && (i == 0 || !is_tag_char(chars[i - 1])) {
            let start = i + 1;
            let mut end = start;
            while end < chars.len() && is_tag_char(chars[end]) {
                end += 1;
            }
            if end > start {
                let tag: String = chars[start..end].iter().collect();
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
    tags
}

/// `body` with every `#from` tag (whole tag only, by the same rule as
/// `extract`) rewritten to `#to`, or `None` if it had none. `#from-x` or
/// `a#from` are left alone — they aren't the tag `from`.
pub fn rename_in_body(body: &str, from: &str, to: &str) -> Option<String> {
    let chars: Vec<char> = body.chars().collect();
    let from_chars: Vec<char> = from.chars().collect();
    let mut out = String::with_capacity(body.len());
    let mut changed = false;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '#' && (i == 0 || !is_tag_char(chars[i - 1])) {
            let start = i + 1;
            let mut end = start;
            while end < chars.len() && is_tag_char(chars[end]) {
                end += 1;
            }
            if chars[start..end] == from_chars[..] {
                out.push('#');
                out.push_str(to);
                changed = true;
                i = end;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    changed.then_some(out)
}

/// Whether `name` is usable as a tag as-is: non-empty, tag characters only.
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(is_tag_char)
}

fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_ascii_and_unicode_tags() {
        assert_eq!(extract("todo #work and #仕事 stuff"), vec!["work", "仕事"]);
    }

    #[test]
    fn dedups_and_ignores_a_bare_hash() {
        assert_eq!(extract("# not a tag, #dup #dup"), vec!["dup"]);
    }

    #[test]
    fn renames_whole_tags_only() {
        let body = "#rust と #rust-lang、a#rust、#rust
#Rust";
        assert_eq!(
            rename_in_body(body, "rust", "Rust").as_deref(),
            Some("#Rust と #rust-lang、a#rust、#Rust
#Rust")
        );
        assert_eq!(rename_in_body("no tags here", "rust", "x"), None);
    }

    #[test]
    fn ignores_a_hash_inside_a_word() {
        assert_eq!(extract("a#b #real"), vec!["real"]);
    }
}
