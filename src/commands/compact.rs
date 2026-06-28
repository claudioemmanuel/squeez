//! `squeez compact-summary` — post-compact dense state re-injection (#166).
//!
//! Conversation history is the biggest token sink in long sessions, and
//! squeez's per-tool hooks can't reach it. After every `/compact`, Claude
//! Code loses concrete state — which files it touched, which errors it hit,
//! recent git refs — and re-discovers it with fresh tool calls.
//!
//! A PreCompact hook can't steer the built-in summarizer (researched: no
//! custom-instructions / transcript-rewrite API; it can only block). But a
//! **PostCompact** hook can return `additionalContext` that survives into the
//! freshly-compacted context. So squeez re-injects its own accumulated session
//! state — which it already tracks in `SessionContext` — as a dense block,
//! plus pointers to any `squeez_retrieve` blobs holding outputs that
//! compaction dropped.
//!
//! Output is the Claude Code PostCompact hook JSON shape; the hook script
//! prints it on stdout.

use crate::context::cache::SessionContext;
use crate::context::retrieve;
use crate::session;

/// Build the post-compact `additionalContext` text from current session state.
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
            .map(|f| format!("{}({})", f.path, f.access.as_char()))
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

    // Retrievable blobs for outputs compaction may have dropped.
    let ids = retrieve::recent_ids(3);
    if !ids.is_empty() {
        parts.push(format!(
            "retrievable: call squeez_retrieve with key in [{}]",
            ids.join(", ")
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
    Some(format!(
        "[squeez session state — restored after compaction] {}",
        parts.join("; ")
    ))
}

fn trim_snippet(s: &str) -> String {
    let one_line = s.replace('\n', " ");
    let t = one_line.trim();
    const MAX: usize = 80;
    if t.chars().count() <= MAX {
        t.to_string()
    } else {
        let cut: String = t.chars().take(MAX).collect();
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
}

/// Emit the PostCompact hook JSON on stdout. Always exits 0 — re-injection is
/// best-effort and must never disrupt the host.
pub fn run() -> i32 {
    if let Some(text) = build_summary() {
        println!(
            "{{\"hookSpecificOutput\":{{\"hookEventName\":\"PostCompact\",\"additionalContext\":\"{}\"}}}}",
            crate::json_util::escape_str(&text)
        );
    }
    0
}
