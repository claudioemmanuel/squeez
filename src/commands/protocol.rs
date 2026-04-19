//! Auto-teach payload that documents squeez to the LLM consuming its output.
//!
//! Ships a hardcoded "memory protocol" + marker spec inside the response so
//! the calling LLM learns the rules of the system on first contact rather than
//! via configuration files.
//!
//! These constants are intentionally short — they live inside every status /
//! protocol response, so any bloat is paid on every call. Keep under ~1500
//! characters combined.

/// High-level protocol the LLM should follow when reading squeez output.
pub const SQUEEZ_PROTOCOL: &str = "\
squeez protocol (read once per session):

1. `# squeez [...]` header — compression intensity, token delta, elapsed time.
2. `[squeez: identical to <hash> at bash#N]` — exact output match from call N.
   Do not re-run.
3. `[squeez: ~P% similar to <hash> at bash#N]` — near-match (Jaccard ≥0.85).
   Same content unless diff needed.
4. `squeez:summary` — dense summaries for huge outputs. Original at
   `~/.claude/squeez/sessions/{date}.jsonl`. Errors + last 20 lines verbatim.
5. MCP tools (use before re-running):
   Context: `squeez_recent_calls` `squeez_seen_files` `squeez_seen_errors`
   `squeez_seen_error_details` `squeez_context_pressure`
   Session: `squeez_session_summary` `squeez_session_detail` `squeez_session_stats`
   `squeez_session_efficiency` `squeez_agent_costs`
   History: `squeez_prior_summaries` `squeez_search_history` `squeez_file_history`
   `squeez_protocol`
6. Context critical (≥75% budget or ≤10 calls left): write `.claude/session_state.md`
   (## Current Objective / ## Files Read / ## Decisions / ## Next Steps) then
   `/clear` or `/compact [focus]`. State file ~2K tokens vs 10K+ for compaction.
";

/// Specification of the structured markers squeez may inject into output.
pub const SQUEEZ_MARKERS_SPEC: &str = "\
markers:

* `# squeez [cmd] IN→OUT (-N%) Tms [adaptive: Lite|Full|Ultra]` — Header.
* `[squeez: identical to <hash> at bash#N]` — Exact redundancy hit.
* `[squeez: ~P% similar to <hash> at bash#N]` — Fuzzy match (Jaccard ≥0.85).
* `squeez:summary cmd=<short>` — Dense summary (total_lines, unique_files,
  errors, test_summary, tail_preserved=N + verbatim last N lines).
* `# squeez hint: <path> (Read tool, call #N)` — File already loaded.
* `[squeez: sig-mode N signatures from K lines]` — AST extraction. Use sed.
* `# squeez: <file> ~T tokens — memory file over 1KB limit` — Size warning.
* `{\"squeez\":\"summary\",...}` — JSON envelope (cmd, total, files, errors).
";

/// Combined payload returned by the MCP `squeez_protocol` tool and by
/// `squeez status --for-llm`. Single allocation, no formatting at call time.
pub fn full_payload() -> String {
    let mut s = String::with_capacity(SQUEEZ_PROTOCOL.len() + SQUEEZ_MARKERS_SPEC.len() + 2);
    s.push_str(SQUEEZ_PROTOCOL);
    s.push('\n');
    s.push_str(SQUEEZ_MARKERS_SPEC);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_is_nonempty() {
        assert!(SQUEEZ_PROTOCOL.len() > 200);
        assert!(SQUEEZ_MARKERS_SPEC.len() > 200);
    }

    #[test]
    fn full_payload_includes_both() {
        let p = full_payload();
        assert!(p.contains("squeez protocol"));
        assert!(p.contains("markers:"));
    }

    #[test]
    fn payload_under_3kb() {
        // Hard ceiling so this never bloats unnoticed. Current size ~2.4 KB.
        // Bump deliberately if you add a documented marker; do not bump
        // because of phrasing creep.
        let n = full_payload().len();
        assert!(n < 3072, "payload too large: {} bytes", n);
    }

    #[test]
    fn payload_documents_all_mcp_tools() {
        let p = full_payload();
        for tool in [
            "squeez_recent_calls",
            "squeez_seen_files",
            "squeez_seen_errors",
            "squeez_seen_error_details",
            "squeez_context_pressure",
            "squeez_session_summary",
            "squeez_session_detail",
            "squeez_session_stats",
            "squeez_session_efficiency",
            "squeez_agent_costs",
            "squeez_prior_summaries",
            "squeez_search_history",
            "squeez_file_history",
            "squeez_protocol",
        ] {
            assert!(p.contains(tool), "payload missing mention of {}", tool);
        }
    }

    #[test]
    fn payload_documents_us001_sig_mode_marker() {
        // US-001: sig-mode marker documents AST signature extraction.
        let p = full_payload();
        assert!(
            p.contains("sig-mode"),
            "payload missing sig-mode marker from US-001"
        );
    }

    #[test]
    fn payload_documents_us002_memory_file_warning() {
        // US-002: memory-file size warning marker.
        // Marker contains a glyph and file path reference.
        let p = full_payload();
        assert!(
            p.contains("memory file") && p.contains("1KB"),
            "payload missing memory-file warning marker from US-002"
        );
    }

    #[test]
    fn payload_documents_us003_structured_summary_json() {
        // US-003: structured summary JSON envelope marker.
        let p = full_payload();
        assert!(
            p.contains("\"squeez\":\"summary\""),
            "payload missing structured summary JSON marker from US-003"
        );
    }
}
