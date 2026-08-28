//! Inline `#tag` extraction. Hand-rolled rather than pulling in `regex` —
//! matches the house preference (see Rclip's memchr-based search) for a scan
//! this simple not to need a dependency, and stays Unicode-aware so
//! `#仕事`-style tags work.

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
    fn ignores_a_hash_inside_a_word() {
        assert_eq!(extract("a#b #real"), vec!["real"]);
    }
}
