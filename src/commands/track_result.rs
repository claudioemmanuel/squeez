// PostToolUse observer — extracts artifacts (file paths, errors) and feeds
// them into SessionContext so future calls can dedup against already-seen
// state. Output rewriting (updatedToolOutput) is handled by compress_output.rs,
// which runs as a separate step in posttooluse.sh for Read/Grep/Glob.

use std::io::Read;
use std::path::Path;

use crate::commands::wrap;
use crate::config::Config;
use crate::context::cache::SessionContext;
use crate::session;

const MAX_CONTENT_BYTES: usize = 256 * 1024;

pub fn run(tool: &str) -> i32 {
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        return 0; // never block
    }
    let dir = session::sessions_dir();
    run_with_dir(tool, &buf, &dir)
}

pub fn run_with_dir(tool: &str, raw: &str, sessions_dir: &Path) -> i32 {
    run_with_dir_cfg(tool, raw, sessions_dir, &Config::load())
}

pub fn run_with_dir_cfg(tool: &str, raw: &str, sessions_dir: &Path, cfg: &Config) -> i32 {
    if raw.trim().is_empty() {
        return 0;
    }

    // Parse minimal JSON via existing helpers (flat keys only).
    let file_path = extract_string_field(raw, "file_path");
    let pattern = extract_string_field(raw, "pattern");
    let path_arg = extract_string_field(raw, "path");
    // SubagentStop wraps last_assistant_message as tool_result.content in the
    // hook script, so extract_content() already handles it. For direct calls
    // (e.g. tests) also check last_assistant_message at the top level.
    let content = extract_content(raw)
        .or_else(|| extract_string_field(raw, "last_assistant_message").map(|s| unescape(&s)));

    let mut ctx = SessionContext::load(sessions_dir);

    // Real-context sync (transcript audit CF-1): the hook payload carries the
    // session transcript path; its last assistant record's usage block is the
    // measured context size. This is what lets adaptive intensity fire in
    // MCP/image-heavy sessions whose tokens never pass through squeez.
    if let Some(tp) = extract_string_field(raw, "transcript_path") {
        let tp = unescape(&tp);
        let path = Path::new(&tp);
        if let Some(tokens) = crate::context::transcript::last_context_tokens(path) {
            ctx.real_ctx_tokens = tokens;
        }
        if let Some((cr, _io)) = crate::context::transcript::last_context_cache_ratio(path) {
            ctx.real_cache_read_tokens = cr;
        }
        // Detect the host's real context window so budget/pressure math keys
        // off the actual window (e.g. 1M) instead of the legacy 112.5K default
        // (CF-2). Combine the model-id baseline with the largest observed
        // context: any context >200K can only be the 1M tier. `transcript_path`
        // is read above, so `real_ctx_tokens` is already current here.
        let model_window = crate::context::transcript::detect_window(path).unwrap_or(0);
        // Monotonic: a window never shrinks mid-session, and real_ctx_tokens
        // fluctuates turn-to-turn (cache dynamics). Once any turn proves the 1M
        // tier (>200K observed), stay there rather than dropping back to 200K.
        let inferred =
            crate::context::transcript::infer_window(model_window, ctx.real_ctx_tokens);
        ctx.real_ctx_window = ctx.real_ctx_window.max(inferred);
    }

    // Bump the call counter for context tracking; tool calls advance state too.
    ctx.next_call_n();
    // Update idle-cache timestamp on every track_result invocation so the
    // 5-min ephemeral cache guard in wrap.rs has an accurate last-activity ts.
    ctx.last_activity_ts = crate::session::unix_now();
    // G2: Call-rate spike detection — warn once if ≥ call_rate_warn_per_min
    // calls fire within a 60-second window (indicates agent loop or runaway workflow).
    if cfg.call_rate_warn_per_min > 0 {
        let rate = ctx.tick_call_rate();
        if rate >= cfg.call_rate_warn_per_min
            && !ctx.nudged_keys.iter().any(|k| k == "call_rate_warn")
        {
            ctx.nudged_keys.push("call_rate_warn".to_string());
            ctx.queue_warning(&format!(
                "HIGH CALL RATE — {} tool calls in 60s — possible agent loop; \
                 check workflow stop conditions",
                rate
            ));
        }
    }

    // Repeated-screenshot advisory (S3.2): navigating to an already-loaded page
    // and re-capturing it is a measured token drain (5/20 shots in the audited
    // session were navigate→screenshot of a URL already open). When the same
    // normalized URL is navigated again within the window, queue a one-shot nudge
    // toward read_page. URL-only; never touches image bytes.
    if cfg.screenshot_repeat_window_secs > 0
        && (tool.contains("navigate") || tool.ends_with("new_page"))
    {
        if let Some(url) = extract_string_field(raw, "url") {
            let url = unescape(&url);
            if let Some(elapsed) = ctx.record_screenshot(
                &url,
                ctx.last_activity_ts,
                cfg.screenshot_repeat_window_secs,
            ) {
                let host_path = crate::context::cache::normalize_url_path(&url);
                ctx.queue_warning(&format!(
                    "revisiting {} ({}s since last visit) — page likely still loaded; \
                     prefer read_page/DOM over a fresh navigate+screenshot",
                    host_path, elapsed
                ));
            }
        }
    }

    // Explicit tool targets carry the tool's access semantics: an Edit/Write
    // target is a WRITE. This powers the A2 stale-state guard in
    // compress_output — a file flagged Write must never have its next re-read
    // fuzzy-dropped (audit item 7).
    let explicit_access = match tool {
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
            crate::context::cache::FileAccess::Write
        }
        _ => crate::context::cache::FileAccess::Read,
    };
    let mut explicit_paths: Vec<String> = Vec::new();
    if let Some(p) = file_path {
        explicit_paths.push(p);
    }
    if let Some(p) = path_arg {
        explicit_paths.push(p);
    }
    for p in &explicit_paths {
        ctx.note_file(p, explicit_access.clone());
        // Sensitive path warning (E7 tier 2): a Read/Grep/Glob target that
        // references a conventionally-sensitive path (.env, ~/.ssh/id_rsa,
        // .netrc, ...) is worth a heads-up even though this observer has no
        // stdout channel of its own -- queued and drained by the next
        // `squeez wrap` bash call, same as the quota/call-rate warnings
        // above. Warn-only, same rationale as wrap.rs's command-string check.
        if let Some(pat) = crate::context::sensitive::path_denylist_match(p) {
            ctx.queue_warning(&format!(
                "path {} references a sensitive-looking path (\"{}\") — verify no secrets before sharing this output",
                p, pat
            ));
        }
    }

    // Files extracted from content body are reads.
    let mut files: Vec<String> = Vec::new();
    if let Some(content) = content.as_deref() {
        let trimmed: String = content.chars().take(MAX_CONTENT_BYTES).collect();
        let mut paths = wrap::extract_file_paths(&trimmed);
        files.append(&mut paths);
        let errors = wrap::extract_errors(&trimmed);
        if !errors.is_empty() {
            // Sensitive guard (Epic E7): `note_errors` persists a 128-char
            // verbatim snippet of every new error line into context.json
            // (surfaced later via the `squeez_seen_error_details` MCP tool).
            // Drop any line that looks like a credential before it reaches
            // that persistence path — independent of the compression
            // decisions above, and default-on (not configurable yet).
            let safe_errors: Vec<String> = errors
                .into_iter()
                .filter(|e| crate::context::sensitive::is_sensitive(e).is_none())
                .collect();
            if !safe_errors.is_empty() {
                ctx.note_errors(&safe_errors);
            }
        }
        // Quota/plan-limit escalation (CF-3): these errors rarely match the
        // `error:`-prefix heuristics above (e.g. Figma's "You've reached the
        // tool call limit on the Starter plan"), and they are never transient
        // — retrying the same tool is guaranteed waste. Scan content directly.
        crate::economy::nudge::note_quota_errors(&mut ctx, tool, &trimmed, cfg);
    }
    if let Some(_p) = pattern {
        // No-op: pattern alone isn't a file fingerprint, but we may want
        // to log it as a tool_artifacts event in the session log.
    }

    // Content often re-mentions the explicit target (Edit preamble echoes the
    // path); don't let a content-derived Read downgrade its Write flag.
    files.retain(|p| !explicit_paths.contains(p));
    if !files.is_empty() {
        ctx.note_files(&files);
    }

    // Track tokens consumed by this tool call (estimate from content length)
    if let Some(ref c) = content {
        let tokens = (c.len() / 4) as u64;
        if tokens > 0 {
            ctx.note_tool_tokens(tool, tokens);
        }
        // Sub-agent oversized-result guard (transcript audit OF-1): a large
        // result returned into the parent rides in the prefix for every
        // remaining turn. Audit case: one 24.5K-token plan ≈ 9% of all
        // cache-read tokens for the session.
        if tool == "SubagentStop"
            && cfg.subagent_result_warn_tokens > 0
            && tokens as usize > cfg.subagent_result_warn_tokens
        {
            ctx.queue_warning(&format!(
                "sub-agent returned ~{}K tokens into the parent context — \
                 prefer writing detail to a file and returning a ≤2K summary + path",
                (tokens / 1000).max(1)
            ));
        }
        // Cross-agent duplicate file reads (audit finding E1 / C2): when
        // multiple parallel agents independently read the same codebase files,
        // each re-creates context for content already available. Detect via
        // the per-agent file map, keyed by agent_id from the hook payload.
        if tool == "SubagentStop" {
            let agent_id = extract_string_field(raw, "agent_id").unwrap_or_default();
            if !agent_id.is_empty() && !files.is_empty() {
                let all_paths: Vec<String> = explicit_paths
                    .iter()
                    .chain(files.iter())
                    .cloned()
                    .collect();
                let dups = ctx.note_subagent_files(&agent_id, &all_paths);
                if !dups.is_empty() {
                    let preview: Vec<&str> = dups.iter().take(3)
                        .map(|p| p.rsplit('/').next().unwrap_or(p.as_str()))
                        .collect();
                    let extra = if dups.len() > 3 {
                        format!(" +{} more", dups.len() - 3)
                    } else {
                        String::new()
                    };
                    ctx.queue_warning(&format!(
                        "cross-agent dup — {} file(s) already read by a sibling agent \
                         ({}{}) — pass pre-read content via args to avoid redundant reads",
                        dups.len(),
                        preview.join(", "),
                        extra
                    ));
                }
            }
            // Cumulative subagent output warning (audit finding E1/D1): fires
            // once per session when the total subagent token volume is high.
            if cfg.subagent_total_warn_tokens > 0 {
                let cumulative: u64 = ctx.agent_spawn_log
                    .iter()
                    .map(|e| e.estimated_tokens)
                    .sum::<u64>()
                    .saturating_add(tokens);
                if cumulative > cfg.subagent_total_warn_tokens
                    && !ctx.pending_warnings.iter()
                        .any(|w| w.contains("cumulative sub-agent output"))
                    && !ctx.nudged_keys.iter()
                        .any(|k| k == "subagent_total_warn")
                {
                    ctx.nudged_keys.push("subagent_total_warn".to_string());
                    ctx.queue_warning(&format!(
                        "cumulative sub-agent output ~{}K tokens this session — \
                         consider reducing parallelism or passing shared context via args",
                        cumulative / 1000
                    ));
                }
            }
        }
    }

    ctx.save(sessions_dir);
    0
}

/// Extract a string value from the raw input, scanning the whole document
/// for any `"key":"value"` occurrence (nested or flat).
fn extract_string_field(raw: &str, key: &str) -> Option<String> {
    let pat = format!("\"{}\":", key);
    let mut rest = raw;
    while let Some(idx) = rest.find(&pat) {
        let after = &rest[idx + pat.len()..];
        let after = after.trim_start();
        if let Some(stripped) = after.strip_prefix('"') {
            // simple string value
            if let Some(end) = stripped.find('"') {
                let val = &stripped[..end];
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
        rest = &rest[idx + pat.len()..];
    }
    None
}

/// Extract `tool_result.content` (or top-level `content`) as a single string.
/// Handles both `"content":"…"` and `"content":[{"type":"text","text":"…"}]`.
fn extract_content(raw: &str) -> Option<String> {
    // Try plain string first
    if let Some(s) = extract_string_field(raw, "content") {
        if !s.trim().is_empty() {
            return Some(unescape(&s));
        }
    }
    // Try `"text":"…"` (Anthropic content-block format)
    let mut out = String::new();
    let mut rest = raw;
    while let Some(idx) = rest.find("\"text\":") {
        let after = &rest[idx + 7..];
        let after = after.trim_start();
        if let Some(stripped) = after.strip_prefix('"') {
            if let Some(end) = stripped.find('"') {
                out.push_str(&unescape(&stripped[..end]));
                out.push('\n');
            }
        }
        rest = &rest[idx + 7..];
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn unescape(s: &str) -> String {
    s.replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static CTR: AtomicU64 = AtomicU64::new(0);
        let n = CTR.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!(
            "squeez_track_result_{}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            n
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn read_input_records_file_path() {
        let dir = tmp();
        let json = r#"{"tool_name":"Read","tool_input":{"file_path":"/tmp/foo.rs"},"tool_result":{"content":"some content"}}"#;
        let rc = run_with_dir("Read", json, &dir);
        assert_eq!(rc, 0);
        let ctx = SessionContext::load(&dir);
        assert!(ctx.seen_files.iter().any(|f| f.path == "/tmp/foo.rs"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn grep_input_no_panic_on_pattern() {
        let dir = tmp();
        let json = r#"{"tool_name":"Grep","tool_input":{"pattern":"fn main","glob":"*.rs"}}"#;
        assert_eq!(run_with_dir("Grep", json, &dir), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_json_exits_zero() {
        let dir = tmp();
        assert_eq!(run_with_dir("Read", "not json at all", &dir), 0);
        assert_eq!(run_with_dir("Read", "", &dir), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extracts_errors_from_content() {
        let dir = tmp();
        let json = r#"{"tool_name":"Read","tool_input":{"file_path":"/tmp/x.log"},"tool_result":{"content":"error: cannot find symbol\nok line"}}"#;
        run_with_dir("Read", json, &dir);
        let ctx = SessionContext::load(&dir);
        assert!(!ctx.seen_errors.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extracts_paths_from_content_body() {
        let dir = tmp();
        let json = r#"{"tool_name":"Bash","tool_result":{"content":"modified: src/main.rs\nmodified: src/lib.rs"}}"#;
        run_with_dir("Bash", json, &dir);
        let ctx = SessionContext::load(&dir);
        assert!(ctx.seen_files.iter().any(|f| f.path == "src/main.rs"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn transcript_path_syncs_real_context() {
        let dir = tmp();
        // Write a fake transcript whose last assistant turn shows 184_620 ctx.
        let transcript = dir.join("session.jsonl");
        std::fs::write(
            &transcript,
            r#"{"type":"assistant","message":{"usage":{"input_tokens":120,"cache_read_input_tokens":180000,"cache_creation_input_tokens":4500}}}"#,
        )
        .unwrap();
        let json = format!(
            r#"{{"tool_name":"Read","transcript_path":"{}","tool_input":{{"file_path":"/tmp/foo.rs"}},"tool_result":{{"content":"x"}}}}"#,
            transcript.display()
        );
        run_with_dir_cfg("Read", &json, &dir, &Config::default());
        let ctx = SessionContext::load(&dir);
        assert_eq!(ctx.real_ctx_tokens, 184_620);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repeated_navigate_queues_screenshot_advisory() {
        let dir = tmp();
        let cfg = Config::default(); // screenshot_repeat_window_secs = 300
        let json = r#"{"tool_name":"mcp__claude-in-chrome__navigate","tool_input":{"url":"https://app.co/dash"}}"#;
        // First navigate: records, no warning.
        run_with_dir_cfg("mcp__claude-in-chrome__navigate", json, &dir, &cfg);
        let ctx = SessionContext::load(&dir);
        assert!(ctx.pending_warnings.is_empty(), "first visit must be silent");
        // Second navigate to the same URL (within window) → one advisory.
        run_with_dir_cfg("mcp__claude-in-chrome__navigate", json, &dir, &cfg);
        let ctx = SessionContext::load(&dir);
        assert_eq!(ctx.pending_warnings.len(), 1, "expected one advisory: {:?}", ctx.pending_warnings);
        assert!(ctx.pending_warnings[0].contains("app.co/dash"));
        assert!(ctx.pending_warnings[0].contains("read_page"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn subagent_big_result_queues_warning() {
        let dir = tmp();
        let cfg = Config::default(); // subagent_result_warn_tokens = 3000
        // ~16K chars ≈ 4K tokens > 3K threshold.
        let big = "word ".repeat(3_300);
        let json = format!(
            r#"{{"tool_name":"SubagentStop","tool_result":{{"content":"{}"}}}}"#,
            big.trim()
        );
        run_with_dir_cfg("SubagentStop", &json, &dir, &cfg);
        let ctx = SessionContext::load(&dir);
        assert_eq!(ctx.pending_warnings.len(), 1);
        assert!(ctx.pending_warnings[0].contains("sub-agent returned"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn subagent_small_result_no_warning() {
        let dir = tmp();
        let cfg = Config::default();
        let json = r#"{"tool_name":"SubagentStop","tool_result":{"content":"done: 3 files reviewed, no issues"}}"#;
        run_with_dir_cfg("SubagentStop", json, &dir, &cfg);
        let ctx = SessionContext::load(&dir);
        assert!(ctx.pending_warnings.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mcp_quota_error_queues_warning_on_second_hit() {
        let dir = tmp();
        let cfg = Config::default(); // quota_error_threshold = 2
        let json = r#"{"tool_name":"mcp__plugin_figma_figma__get_screenshot","tool_result":{"content":"You've reached the Figma MCP tool call limit on the Starter plan."}}"#;
        run_with_dir_cfg("mcp__plugin_figma_figma__get_screenshot", json, &dir, &cfg);
        run_with_dir_cfg("mcp__plugin_figma_figma__get_screenshot", json, &dir, &cfg);
        let ctx = SessionContext::load(&dir);
        assert_eq!(ctx.pending_warnings.len(), 1);
        assert!(ctx.pending_warnings[0].contains("stop retrying"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
