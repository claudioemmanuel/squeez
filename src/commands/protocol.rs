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
/// Five directives, plain English, no jargon.
pub const SQUEEZ_PROTOCOL: &str = "\
squeez memory protocol (read this once per session):

1. Output prefixed with `# squeez [...]` is a header line — it shows the
   compression intensity, token delta, and elapsed time. Not part of the
   command's actual output.
2. Lines containing `[squeez: identical to <hash> at bash#N — re-run with
   --no-squeez]` mean the command produced output that already appeared in
   call #N of this session. Do NOT re-run it; the result is unchanged.
3. Lines containing `[squeez: ~95% similar to <hash> at bash#N]` mean the
   output is near-identical to a prior call (whitespace, timestamps, or one
   edited line). Treat as the same content unless you specifically need the
   diff — re-run with --no-squeez to see verbatim.
4. Lines starting with `squeez:summary` are dense summaries replacing huge
   outputs. The original is in `~/.claude/squeez/sessions/{date}.jsonl`.
   Errors and the last 20 lines are always preserved verbatim.
5. MCP tools — use BEFORE re-running expensive commands or asking the user
   to recall what changed:
   Context:  `squeez_recent_calls` `squeez_seen_files` `squeez_seen_errors`
             `squeez_seen_error_details` `squeez_context_pressure`
   Session:  `squeez_session_summary` `squeez_session_detail`
             `squeez_session_stats` `squeez_session_efficiency`
             `squeez_agent_costs`
   History:  `squeez_prior_summaries` `squeez_search_history`
             `squeez_file_history` `squeez_protocol`

6. Opus 4.7: new tokenizer +35% tokens; xhigh thinking — budget exhausts ~2× faster.
7. When context is critical (≥75% budget or ≤10 calls remaining), write
   `.claude/session_state.md` before clearing:
     ## Current Objective / ## Files Read / ## Decisions Taken / ## Next Steps
   Then `/clear` to reset, or `/compact [describe focus area]` for a
   smaller focused summary. Reading the state file costs ~2K tokens vs
   10K–20K for a compaction summary.
";

/// Specification of the structured markers squeez may inject into output.
/// Designed to be greppable and unambiguous so the LLM can pattern-match.
pub const SQUEEZ_MARKERS_SPEC: &str = "\
squeez output markers:

* `# squeez [cmd] IN→OUT tokens (-N%) Tms [adaptive: Lite|Full|Ultra]`
    Header. IN/OUT are token estimates. `[adaptive: ...]` shows the chosen
    intensity (Full at <65% budget, Ultra at ≥65%, Lite when adaptive off).

* `[squeez: identical to <hash> at bash#N — re-run with --no-squeez]`
    Exact-hash redundancy hit. The output of this call exactly matches an
    earlier call within the last 16 commands.

* `[squeez: ~P% similar to <hash> at bash#N — re-run with --no-squeez]`
    Fuzzy redundancy hit (Jaccard ≥ 0.85 over trigram shingles). Whitespace,
    timestamps, or single-line edits don't break this match.

* `squeez:summary cmd=<short>` followed by `total_lines=N`,
  `unique_files=N`, optional `top_errors:`, `top_files:`,
  `test_summary=...`, `tail_preserved=N`, then the verbatim last N lines.
    Dense summary for outputs over the per-call line threshold.

* `# squeez hint: <path> already in context (Read tool, call #N)`
    Soft hint when `cat`/`head`/`tail`/`less`/`more`/`bat` is invoked on a
    file the Read tool has already loaded earlier in the session.
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
        assert!(p.contains("squeez memory protocol"));
        assert!(p.contains("squeez output markers"));
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
}
