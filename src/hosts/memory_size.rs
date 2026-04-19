//! Memory-file size warning utilities.
//!
//! Estimates token count for a memory file's non-squeez content and returns
//! a banner string when the content exceeds the configured limit.

const SQUEEZ_START: &str = "<!-- squeez:start -->";
const SQUEEZ_END: &str = "<!-- squeez:end -->";

/// Rough chars-per-token heuristic (4 chars ≈ 1 token).
pub fn estimate_tokens(s: &str) -> usize {
    s.len() / 4
}

/// Strip any existing `<!-- squeez:start --> ... <!-- squeez:end -->` block
/// before measuring — the squeez block's tokens don't count against the
/// user's budget.
fn strip_squeez_block(s: &str) -> &str {
    if !s.contains(SQUEEZ_START) {
        return s;
    }
    // We only strip a leading block (the block is always written first).
    // Fall back to full string on malformed input.
    if let Some(start) = s.find(SQUEEZ_START) {
        if let Some(end_offset) = s.find(SQUEEZ_END) {
            let end = end_offset + SQUEEZ_END.len();
            // skip the newline that follows `<!-- squeez:end -->`
            let rest_start = if end < s.len() && s.as_bytes()[end] == b'\n' {
                end + 1
            } else {
                end
            };
            // Only strip when the block starts at or before the first char of
            // user content (i.e. block is a prefix).
            if start == 0 {
                return &s[rest_start.min(s.len())..];
            }
        }
    }
    s
}

/// Returns `Some(banner)` when `existing_content` (minus any squeez block)
/// exceeds `limit_tokens` tokens.
///
/// * `limit_tokens == 0` → disabled, always returns `None`.
pub fn size_warning(
    existing_content: &str,
    filename: &str,
    limit_tokens: usize,
) -> Option<String> {
    if limit_tokens == 0 {
        return None;
    }
    let user_content = strip_squeez_block(existing_content);
    let n = estimate_tokens(user_content);
    if n <= limit_tokens {
        return None;
    }
    Some(format!(
        "⚠️  {filename} is ~{n} tokens (> {limit_tokens} recommended) — docs suggest <1k tokens for efficient context. Consider splitting project details into on-demand docs.\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_chars_div_4() {
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcdefgh"), 2);
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn no_warning_for_small_content() {
        let content = "a".repeat(100); // 25 tokens
        assert!(size_warning(&content, "CLAUDE.md", 1000).is_none());
    }

    #[test]
    fn warning_for_large_content() {
        let content = "a".repeat(5000); // 1250 tokens
        let result = size_warning(&content, "CLAUDE.md", 1000);
        assert!(result.is_some());
        let banner = result.unwrap();
        assert!(banner.contains("CLAUDE.md"));
        assert!(banner.contains("1250"));
        assert!(banner.contains("1000"));
    }

    #[test]
    fn disabled_when_limit_zero() {
        let content = "a".repeat(9999);
        assert!(size_warning(&content, "CLAUDE.md", 0).is_none());
    }

    #[test]
    fn squeez_block_not_counted() {
        // Large squeez block + tiny user content — should not warn.
        let squeez_block = format!(
            "<!-- squeez:start -->\n{}\n<!-- squeez:end -->\n",
            "x".repeat(8000)
        );
        let content = format!("{}\nhi\n", squeez_block);
        // user content is just "\nhi\n" → well under 1000 tokens
        assert!(size_warning(&content, "CLAUDE.md", 1000).is_none());
    }

    #[test]
    fn squeez_block_stripped_then_large_user_content_warns() {
        let squeez_block =
            "<!-- squeez:start -->\nsome persona text\n<!-- squeez:end -->\n".to_string();
        let user_content = "y".repeat(8000);
        let content = format!("{}{}", squeez_block, user_content);
        let result = size_warning(&content, "AGENTS.md", 1000);
        assert!(result.is_some());
        let banner = result.unwrap();
        assert!(banner.contains("AGENTS.md"));
    }
}
