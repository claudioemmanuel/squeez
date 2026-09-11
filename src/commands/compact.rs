//! `squeez compact-summary` — post-compact dense state re-injection (#166).
//!
//! Conversation history is the biggest token sink in long sessions, and
//! squeez's per-tool hooks can't reach it. After every `/compact`, Claude
//! Code loses concrete state — which files it touched, which errors it hit,
//! recent git refs — and re-discovers it with fresh tool calls.
//!
//! A PreCompact hook can't steer the built-in summarizer (researched: no
//! custom-instructions / transcript-rewrite API; it can only block). So squeez
//! re-injects its own accumulated session state — which it already tracks in
//! `SessionContext` — as a dense block, plus pointers to any `squeez_retrieve`
//! blobs holding outputs that compaction dropped.
//!
//! Delivery goes through **SessionStart** (`source: "compact"`), which fires
//! right after the compaction summary is written and whose stdout becomes
//! context. PostCompact cannot inject anything: Claude Code rejects
//! `hookSpecificOutput` for it and shows its stdout only as a UI notice
//! (#225). Output is therefore plain text, never hook JSON.

use crate::context::cache::SessionContext;
use crate::context::retrieve;
use crate::session;

/// Build the post-compact context text from current session state.
/// Returns `None` when there's nothing worth re-injecting.
pub fn build_summary() -> Option<String> {
    let ctx = SessionContext::load(&session::sessions_dir());
    let cur = session::CurrentSession::load(&session::sessions_dir());

    let mut parts: Vec<String> = Vec::new();

    // Recently-touched files (newest first), with access mode.
    if !ctx.seen_files.is_empty() {
        let mut files = ctx.seen_files.clone();
        files.sort_by(|a, b| b.last_seen_call.cmp(&a.last_seen_call));
        let listed: Vec<String> = files
            .iter()
            .take(8)
            .map(|f| format!("{}({})", cap_chars(&f.path, 160), f.access.as_char()))
            .collect();
        parts.push(format!("files: {}", listed.join(", ")));
    }

    // Distinct error snippets (most recent first), trimmed.
    if !ctx.error_snippets.is_empty() {
        let listed: Vec<String> = ctx
            .error_snippets
            .iter()
            .rev()
            .take(3)
            .map(|(_, snip)| trim_snippet(snip))
            .collect();
        parts.push(format!("errors: {}", listed.join(" | ")));
    }

    // Recent git refs.
    if !ctx.seen_git_refs.is_empty() {
        let refs: Vec<String> = ctx.seen_git_refs.iter().rev().take(5).cloned().collect();
        parts.push(format!("git: {}", refs.join(", ")));
    }

    // Retrievable blobs for outputs compaction may have dropped. Each key is
    // annotated with its top distinctive terms (E4) so the model can tell
    // what a key is about without a squeez_retrieve round trip.
    let ids = retrieve::recent_ids(3);
    if !ids.is_empty() {
        let annotated: Vec<String> = ids
            .iter()
            .map(|id| {
                let terms = retrieve::terms_for(id, 3);
                if terms.is_empty() {
                    id.clone()
                } else {
                    format!("{}({})", id, terms.join(","))
                }
            })
            .collect();
        parts.push(format!(
            "retrievable: call squeez_retrieve with key in [{}]",
            annotated.join(", ")
        ));
    }

    if let Some(c) = cur {
        if c.tokens_saved > 0 {
            parts.push(format!(
                "squeez saved ~{}tk over {} calls this session",
                c.tokens_saved, c.total_calls
            ));
        }
    }

    if parts.is_empty() {
        return None;
    }
    let text = format!(
        "[squeez session state — restored after compaction] {}",
        parts.join("; ")
    );
    // Hard ceiling: this lands in the context right after compaction reclaimed
    // it, and a state summary never legitimately needs more. Any future
    // escaping regression (#229) then costs a few KB, not millions of tokens.
    Some(cap_chars(&text, MAX_SUMMARY_CHARS))
}

/// Upper bound on the re-injected summary, in chars.
const MAX_SUMMARY_CHARS: usize = 4000;

fn trim_snippet(s: &str) -> String {
    cap_chars(s.replace('\n', " ").trim(), 80)
}

fn cap_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_snippet_collapses_newlines_and_caps_length() {
        assert_eq!(trim_snippet("  a\nb  "), "a b");
        let long = "x".repeat(200);
        let out = trim_snippet(&long);
        assert!(out.ends_with('…'));
        assert!(out.chars().count() <= 81);
    }

    #[test]
    fn cap_chars_bounds_oversized_text() {
        let huge = "\\".repeat(1 << 20);
        let out = cap_chars(&huge, MAX_SUMMARY_CHARS);
        assert_eq!(out.chars().count(), MAX_SUMMARY_CHARS + 1);
        assert_eq!(cap_chars("short", MAX_SUMMARY_CHARS), "short");
    }

    #[test]
    fn only_the_compact_source_counts_as_a_compact_start() {
        assert!(is_compact_start(r#"{"session_id":"s","source":"compact"}"#));
        assert!(is_compact_start(r#"{"source": "compact"}"#));
        for other in ["startup", "resume", "clear"] {
            assert!(!is_compact_start(&format!(r#"{{"source":"{other}"}}"#)));
        }
        assert!(!is_compact_start(""));
    }
}

/// Whether a SessionStart hook payload is the restart that follows `/compact`.
pub fn is_compact_start(payload: &str) -> bool {
    crate::json_util::extract_str(payload, "source").as_deref() == Some("compact")
}

/// `squeez compact-summary --session-start`: the SessionStart hook entry.
/// Reads the hook payload from stdin and restores state only when this start
/// follows a compaction; every other start prints nothing.
pub fn run_session_start() -> i32 {
    use std::io::{IsTerminal, Read};
    let mut stdin = std::io::stdin();
    if stdin.is_terminal() {
        return 0;
    }
    let mut payload = String::new();
    let _ = stdin.read_to_string(&mut payload);
    if is_compact_start(&payload) {
        run()
    } else {
        0
    }
}

/// Print the session-state summary as plain text on stdout. Always exits 0 —
/// re-injection is best-effort and must never disrupt the host.
pub fn run() -> i32 {
    if let Some(text) = build_summary() {
        println!("{text}");
    }
    // The header tag-dedup memo (E1) tracks what the model has already seen;
    // compaction rebuilds the model's context from scratch, so the memo must
    // reset or an unchanged budget/agent tag would stay suppressed even
    // though the model no longer holds the prior header that set it.
    let sessions_dir = session::sessions_dir();
    let mut ctx = SessionContext::load(&sessions_dir);
    ctx.reset_header_tag_memo();
    ctx.save(&sessions_dir);
    0
}
