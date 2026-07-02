// PostToolUse rewrite hook — Claude Code v2.1.119+ added `updatedToolOutput`
// to PostToolUse for all tools, not just MCP. This command reads the
// PostToolUse JSON from stdin, checks the content against SessionContext for
// exact/fuzzy redundancy, and prints a hookSpecificOutput JSON blob when the
// content can be compressed. Exits 0 with no stdout when no rewrite is needed
// (Claude Code sees the original result unchanged).
//
// Called from hooks/posttooluse.sh for Read / Grep / Glob / Edit / Write
// tool calls. Bash compression is handled at PreToolUse via `squeez wrap`.

use std::io::Read;
use std::path::Path;

use crate::config::Config;
use crate::context;
use crate::session;

const MAX_CONTENT_BYTES: usize = 256 * 1024;
/// Minimum byte saving required before emitting an Edit/Write preamble-strip
/// rewrite. Smaller wins aren't worth the rewrite plumbing overhead.
const EDIT_STRIP_MIN_SAVINGS: usize = 30;

pub fn run(tool: &str) -> i32 {
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        return 0;
    }
    let dir = session::sessions_dir();
    let cfg = Config::load();
    run_with(&buf, tool, &dir, &cfg)
}

pub fn run_with(raw: &str, tool: &str, sessions_dir: &Path, cfg: &Config) -> i32 {
    if let Some(out) = compute_rewrite(raw, tool, sessions_dir, cfg) {
        println!("{}", render_updated_output(raw, tool, &out));
    }
    0
}

/// Core logic: returns the rewritten content string if compression applies,
/// or `None` if the original should be kept as-is. Exposed for testing.
pub fn compute_rewrite(raw: &str, tool: &str, sessions_dir: &Path, cfg: &Config) -> Option<String> {
    if raw.trim().is_empty() {
        return None;
    }

    // Image payloads (Read of an image file, MCP screenshots) carry base64 in
    // a "data" field and no text content, so the text paths below never see
    // them — the audited session emitted zero dedup markers across repeated
    // identical screenshots. Byte-identical images already in context are safe
    // to omit (transcript audit item 3); near-identical ones are not.
    if let Some(rewrite) = image_dedup(raw, sessions_dir, cfg) {
        return Some(rewrite);
    }

    let raw_content = extract_content(raw).filter(|c| !c.trim().is_empty())?;
    let raw_content: String = raw_content.chars().take(MAX_CONTENT_BYTES).collect();

    // Edit/Write: strip the verbose cat -n preamble that Claude Code emits.
    // Saves ~70-100B per call across many edits; only applied when the gain
    // exceeds EDIT_STRIP_MIN_SAVINGS to avoid noisy rewrites for tiny results.
    if cfg.strip_edit_preamble && (tool == "Edit" || tool == "Write") {
        if let Some(stripped) = strip_edit_preamble(&raw_content) {
            let saved = raw_content.len().saturating_sub(stripped.len());
            if saved >= EDIT_STRIP_MIN_SAVINGS {
                return Some(stripped);
            }
        }
        // Edit/Write don't participate in redundancy/summarize.
        return None;
    }

    let content = raw_content;
    let lines: Vec<String> = content.lines().map(String::from).collect();

    if lines.is_empty() {
        return None;
    }

    let mut ctx = context::cache::SessionContext::load(sessions_dir);

    // Skill bodies (SKILL.md injected when Claude invokes the Skill tool) are
    // large, structured, and EXACTLY repeatable: the same skill re-injected
    // later in a session is byte-identical. The real win is dropping that
    // duplicate, not lexically abbreviating prose (skill bodies are checklists
    // and code, not prose — lexical compression nets ~1%). Re-injections recur
    // at arbitrary distance, so the windowed `redundancy::check` (last 16 calls)
    // misses them; use the session-long skill-injection store instead.
    if tool == "Skill" {
        if cfg.redundancy_cache_enabled {
            let fp = crate::context::hash::fnv1a_64(content.as_bytes());
            if let Some(call_n) = ctx.skill_dedup_lookup(fp) {
                ctx.exact_dedup_hits += 1;
                ctx.save(sessions_dir);
                return Some(format!(
                    "[squeez: identical to Skill #{call_n} — body omitted]"
                ));
            }
            ctx.skill_dedup_record(fp);
            ctx.save(sessions_dir);
        }
        // First injection: recorded, kept full-size.
        return None;
    }

    // A2 stale-state guard (audit item 7): the latest read of a file that was
    // edited since squeez last saw it must reach the model intact. A fuzzy
    // (~similar) match against the PRE-edit read would drop exactly the new
    // content the model needs. Byte-identical matches stay safe — identical
    // bytes mean nothing changed. Requires posttooluse.sh to run
    // compress-output BEFORE track-result, so the Write flag set by the Edit
    // is still visible here.
    let edited_since_seen = extract_string_field(raw, "file_path")
        .map(|p| unescape(&p))
        .map(|p| {
            ctx.seen_files.iter().any(|f| {
                f.path == p
                    && matches!(
                        f.access,
                        context::cache::FileAccess::Write | context::cache::FileAccess::Created
                    )
            })
        })
        .unwrap_or(false);

    // MCP results (DOM snapshots, evaluate_script returns, structured JSON) are
    // dedup-only, and only EXACT dedup is safe for them: two near-identical
    // snapshots differ precisely in the fields the model asked for (element
    // uids, a grid cell value, coordinates), so a fuzzy "~96% similar" drop
    // omits exactly the payload — the same failure the A2 stale-state guard
    // prevents for edited files. Suppress fuzzy dedup for MCP; byte-identical
    // re-emissions still collapse (identical bytes carry no new information).
    let is_mcp = tool.starts_with("mcp__");

    // Redundancy check: if we've seen this content before, replace with a note.
    if cfg.redundancy_cache_enabled {
        if let Some(hit) = context::redundancy::check(&ctx, &lines) {
            let fuzzy_blocked = hit.similarity.is_some() && (edited_since_seen || is_mcp);
            if !fuzzy_blocked {
                let note = match hit.similarity {
                    None => format!(
                        "[squeez: identical to {} #{} — output omitted]",
                        tool, hit.call_n
                    ),
                    Some(j) => format!(
                        "[squeez: ~{}% similar to {} #{} — re-read if needed]",
                        (j * 100.0).round() as u32,
                        tool,
                        hit.call_n
                    ),
                };
                ctx.exact_dedup_hits += 1;
                ctx.save(sessions_dir);
                return Some(note);
            }
        }
    }

    // Large benign outputs get summarized (Skill is handled earlier via the
    // session-long dedup store and never reaches here). MCP results are
    // dedup-only: they are structured payloads (JSON, DOM snapshots) that the
    // log-oriented summarizer would corrupt, so they participate in the
    // redundancy check above but are never summarized (audit item 2 — track
    // and dedupe MCP traffic, leave first-sight content intact).
    let rewritten = if !is_mcp && context::summarize::should_apply_for_tool(&lines, cfg, tool) {
        let summary = context::summarize::apply(lines.clone(), tool);
        if summary.len() < lines.len() {
            Some(summary.join("\n"))
        } else {
            None
        }
    } else {
        None
    };

    // Record content so future calls can dedup against it.
    if cfg.redundancy_cache_enabled {
        context::redundancy::record(&mut ctx, tool, &lines);
        ctx.save(sessions_dir);
    }

    rewritten
}

/// Session-long exact-hash dedup for image payloads. Returns the replacement
/// note when this exact image (by base64 fingerprint) was already seen this
/// session; records it otherwise. Only byte-identical payloads are replaced —
/// a re-render that differs by one pixel hashes differently and passes through.
fn image_dedup(raw: &str, sessions_dir: &Path, cfg: &Config) -> Option<String> {
    if !cfg.redundancy_cache_enabled {
        return None;
    }
    let data = extract_image_data(raw)?;
    let fp = crate::context::hash::fnv1a_64(data.as_bytes());
    let mut ctx = context::cache::SessionContext::load(sessions_dir);
    if let Some(call_n) = ctx.image_dedup_lookup(fp) {
        ctx.exact_dedup_hits += 1;
        ctx.save(sessions_dir);
        return Some(format!(
            "[squeez: identical image to result #{call_n} (already in context) — omitted]"
        ));
    }
    ctx.image_dedup_record(fp);
    ctx.save(sessions_dir);
    None
}

/// Extract a base64 image payload from an image content block
/// (`{"type":"image","source":{"type":"base64","data":"…"}}`). Requires a
/// plausibly-image-sized base64 run (≥1 KB) of strict base64 charset so that
/// ordinary short "data" string fields never match.
fn extract_image_data(raw: &str) -> Option<&str> {
    let idx = raw.find("\"data\":")?;
    let after = raw[idx + 7..].trim_start();
    let stripped = after.strip_prefix('"')?;
    let end = stripped.find('"')?;
    let data = &stripped[..end];
    let is_b64 = data
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=');
    if data.len() >= 1024 && is_b64 {
        Some(data)
    } else {
        None
    }
}

/// Compress the Edit/Write tool preamble. Claude Code emits the boilerplate
/// `"The file <path> has been updated successfully. Here's the result of
/// running `cat -n` on a snippet of the edited file:"` (or similar for Write),
/// which costs ~70-100 tokens per call. Returns `Some(shortened)` when a
/// strip is possible, `None` if the preamble is missing.
fn strip_edit_preamble(content: &str) -> Option<String> {
    // Look for the verbose phrase. Multiple variants exist depending on
    // tool/version (Edit, Write, NotebookEdit). Match on the most stable
    // anchor: "Here's the result of running `cat -n` on".
    let anchor = "Here's the result of running `cat -n`";
    let idx = content.find(anchor)?;
    // Find end of preamble line (the colon + newline that introduces the snippet).
    let after_anchor = &content[idx..];
    let line_end_rel = after_anchor.find(":\n")?;
    let snippet_start = idx + line_end_rel + 2;
    let snippet = &content[snippet_start..];
    let prefix = content[..idx].trim_end();
    // Keep the leading status line (e.g. "The file X has been updated.") but
    // drop the verbose cat -n explanation.
    Some(format!("{}\n{}", prefix, snippet))
}

/// Claude Code validates `updatedToolOutput` against the rewritten tool's own
/// output schema. Read's output is an object (`{"type":"text","file":{...}}`),
/// so a plain-string rewrite is rejected with "expected object, received
/// string" — the hook warning appears and the compression is discarded. Wrap
/// Read rewrites in that object shape, mirroring the original response's
/// metadata; other rewritten tools accept string output.
fn render_updated_output(raw: &str, tool: &str, content: &str) -> String {
    let escaped = json_escape(content);
    if tool == "Read" {
        // file_path arrives JSON-escaped in tool_input; re-embed verbatim.
        let path = extract_string_field(raw, "file_path").unwrap_or_default();
        let num_lines = content.lines().count().max(1);
        let total_lines = crate::json_util::extract_u64(raw, "totalLines")
            .unwrap_or(num_lines as u64);
        format!(
            r#"{{"hookSpecificOutput":{{"hookEventName":"PostToolUse","updatedToolOutput":{{"type":"text","file":{{"filePath":"{}","content":"{}","numLines":{},"startLine":1,"totalLines":{}}}}}}}}}"#,
            path, escaped, num_lines, total_lines
        )
    } else {
        format!(
            r#"{{"hookSpecificOutput":{{"hookEventName":"PostToolUse","updatedToolOutput":"{}"}}}}"#,
            escaped
        )
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// Extract the tool result content from the PostToolUse JSON.
/// Handles both `"content":"…"` and Anthropic content-block format.
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
            if let Some(end) = find_unescaped_quote(stripped) {
                out.push_str(&unescape(&stripped[..end]));
                out.push('\n');
            }
        }
        rest = &rest[idx + 7..];
    }
    if out.is_empty() { None } else { Some(out) }
}

fn extract_string_field(raw: &str, key: &str) -> Option<String> {
    let pat = format!("\"{}\":", key);
    let mut rest = raw;
    while let Some(idx) = rest.find(&pat) {
        let after = &rest[idx + pat.len()..];
        let after = after.trim_start();
        if let Some(stripped) = after.strip_prefix('"') {
            if let Some(end) = find_unescaped_quote(stripped) {
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

/// Byte offset of the first `"` in `s` that terminates a JSON string — i.e. the
/// first `"` not part of an escape sequence. A naive `find('"')` stops at the
/// escaped quote inside values like `class=\"grid\"`, truncating the content to
/// its pre-first-quote prefix; for DOM snapshots that prefix is constant across
/// calls, so distinct snapshots collide on the exact-hash dedup and get dropped.
fn find_unescaped_quote(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2, // skip the escaped char (\" or \\ etc.)
            b'"' => return Some(i),
            _ => i += 1,
        }
    }
    None
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
    use crate::config::Config;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn read_rewrite_is_object_shaped() {
        let raw = r#"{"tool_name":"Read","tool_input":{"file_path":"/tmp/a.rs"},"tool_response":{"type":"text","file":{"filePath":"/tmp/a.rs","content":"x","numLines":900,"startLine":1,"totalLines":900}}}"#;
        let out = render_updated_output(raw, "Read", "line1\nline2");
        assert!(out.contains(r#""updatedToolOutput":{"type":"text","file":{"filePath":"/tmp/a.rs""#));
        assert!(out.contains(r#""content":"line1\nline2""#));
        assert!(out.contains(r#""numLines":2"#));
        assert!(out.contains(r#""totalLines":900"#));
    }

    #[test]
    fn non_read_rewrite_stays_string() {
        let out = render_updated_output("{}", "Grep", "note");
        assert!(out.contains(r#""updatedToolOutput":"note""#));
    }

    fn tmp() -> std::path::PathBuf {
        static CTR: AtomicU64 = AtomicU64::new(0);
        let n = CTR.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!(
            "squeez_compress_output_{}_{}",
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
    fn empty_input_exits_cleanly() {
        let dir = tmp();
        let cfg = Config::default();
        assert_eq!(run_with("", "Read", &dir, &cfg), 0);
        assert_eq!(run_with("  ", "Read", &dir, &cfg), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_content_field_exits_cleanly() {
        let dir = tmp();
        let cfg = Config::default();
        let json = r#"{"tool_name":"Read","tool_result":{}}"#;
        assert_eq!(run_with(json, "Read", &dir, &cfg), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn json_escape_handles_special_chars() {
        assert_eq!(json_escape("a\"b"), r#"a\"b"#);
        assert_eq!(json_escape("a\nb"), r"a\nb");
        assert_eq!(json_escape("a\\b"), r"a\\b");
    }

    #[test]
    fn small_content_not_compressed() {
        let dir = tmp();
        let cfg = Config::default();
        let json = r#"{"tool_name":"Read","tool_result":{"content":"hello world"}}"#;
        // Small content: no redundancy hit, no summarize → exit 0, no stdout
        assert_eq!(run_with(json, "Read", &dir, &cfg), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn image_json(seed: char) -> String {
        // ≥1KB strict-base64 payload inside an image content block.
        let data: String = std::iter::repeat(seed).take(2048).collect();
        format!(
            r#"{{"tool_name":"mcp__chrome-devtools__take_screenshot","tool_result":{{"content":[{{"type":"image","source":{{"type":"base64","media_type":"image/jpeg","data":"{}"}}}}]}}}}"#,
            data
        )
    }

    #[test]
    fn identical_image_deduped_on_second_sight() {
        let dir = tmp();
        let cfg = Config::default();
        let json = image_json('A');
        // First sight: recorded, passed through unchanged.
        assert_eq!(compute_rewrite(&json, "mcp__chrome-devtools__take_screenshot", &dir, &cfg), None);
        // Second identical payload: replaced with the dedup note.
        let rewrite = compute_rewrite(&json, "mcp__chrome-devtools__take_screenshot", &dir, &cfg);
        let note = rewrite.expect("identical image should dedup");
        assert!(note.contains("identical image"), "note: {}", note);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn different_images_never_dedup() {
        let dir = tmp();
        let cfg = Config::default();
        assert_eq!(compute_rewrite(&image_json('A'), "Read", &dir, &cfg), None);
        // One-char class difference → different fingerprint → pass through.
        assert_eq!(compute_rewrite(&image_json('B'), "Read", &dir, &cfg), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn short_data_field_is_not_an_image() {
        // Ordinary "data" string fields must never trigger the image path.
        let raw = r#"{"tool_result":{"content":"x"},"data":"abc123"}"#;
        assert!(extract_image_data(raw).is_none());
    }

    #[test]
    fn fuzzy_dedup_blocked_for_edited_file() {
        use crate::context::cache::{FileAccess, SessionContext};
        let dir = tmp();
        let cfg = Config::default();

        // 40 lines of content, recorded as a prior Read.
        let original: Vec<String> = (0..40).map(|i| format!("line {} alpha beta gamma", i)).collect();
        let mut ctx = SessionContext::load(&dir);
        ctx.init_tunables_from_config(&cfg);
        crate::context::redundancy::record(&mut ctx, "Read", &original);
        // The file was then edited → Write flag.
        ctx.note_file("/tmp/a2/edited.rs", FileAccess::Write);
        ctx.save(&dir);

        // Re-read after the edit: one line changed → fuzzy-similar, not identical.
        let mut edited = original.clone();
        edited[20] = "line 20 CHANGED delta".to_string();
        let json = format!(
            r#"{{"tool_name":"Read","tool_input":{{"file_path":"/tmp/a2/edited.rs"}},"tool_result":{{"content":"{}"}}}}"#,
            edited.join("\\n")
        );
        let rewrite = compute_rewrite(&json, "Read", &dir, &cfg);
        // A2 guard: the post-edit read must reach the model intact.
        assert!(
            rewrite.is_none() || !rewrite.as_deref().unwrap_or("").contains("similar"),
            "fuzzy dedup must not drop the latest read of an edited file: {:?}",
            rewrite
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fuzzy_dedup_allowed_for_unedited_file() {
        use crate::context::cache::SessionContext;
        let dir = tmp();
        let cfg = Config::default();

        let original: Vec<String> = (0..40).map(|i| format!("line {} alpha beta gamma", i)).collect();
        let mut ctx = SessionContext::load(&dir);
        ctx.init_tunables_from_config(&cfg);
        crate::context::redundancy::record(&mut ctx, "Read", &original);
        ctx.save(&dir);

        // Same near-identical re-read, but the file was never edited →
        // fuzzy dedup stays active (this is the waste case it exists for).
        let mut similar = original.clone();
        similar[20] = "line 20 CHANGED delta".to_string();
        let json = format!(
            r#"{{"tool_name":"Read","tool_input":{{"file_path":"/tmp/a2/unedited.rs"}},"tool_result":{{"content":"{}"}}}}"#,
            similar.join("\\n")
        );
        let rewrite = compute_rewrite(&json, "Read", &dir, &cfg);
        assert!(
            rewrite.as_deref().map(|r| r.contains("similar")).unwrap_or(false),
            "fuzzy dedup should fire for unedited files: {:?}",
            rewrite
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mcp_results_are_never_summarized() {
        let dir = tmp();
        let mut cfg = Config::default();
        cfg.summarize_threshold_lines = 10;
        cfg.read_summarize_threshold_lines = 10;
        // 400 distinct benign lines — far past any summarize threshold.
        let body: String = (0..400)
            .map(|i| format!("row {} value {}", i, i * 7))
            .collect::<Vec<_>>()
            .join("\\n");
        let json = format!(
            r#"{{"tool_name":"mcp__db__query","tool_result":{{"content":"{}"}}}}"#,
            body
        );
        // MCP: dedup-only — first sight must pass through unsummarized.
        assert_eq!(compute_rewrite(&json, "mcp__db__query", &dir, &cfg), None);
        // Identical second result still dedups via the redundancy store.
        let second = compute_rewrite(&json, "mcp__db__query", &dir, &cfg);
        assert!(second.is_some(), "identical MCP result should dedup");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn edit_preamble_is_stripped() {
        let dir = tmp();
        let cfg = Config::default();
        let content = "The file /a/b.rs has been updated successfully. Here's the result of running `cat -n` on a snippet of the edited file:\n     1\thello\n     2\tworld";
        let json = format!(
            r#"{{"tool_name":"Edit","tool_result":{{"content":"{}"}}}}"#,
            content.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
        );
        let rewrite = compute_rewrite(&json, "Edit", &dir, &cfg);
        assert!(rewrite.is_some(), "Edit preamble should be stripped");
        let out = rewrite.unwrap();
        assert!(
            !out.contains("Here's the result of running"),
            "verbose preamble survived: {}",
            out
        );
        assert!(out.contains("hello"), "snippet must remain");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn edit_without_preamble_passes_through() {
        let dir = tmp();
        let cfg = Config::default();
        let json = r#"{"tool_name":"Edit","tool_result":{"content":"File written."}}"#;
        // No verbose preamble → no rewrite emitted.
        assert!(compute_rewrite(json, "Edit", &dir, &cfg).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn edit_strip_disabled_by_config() {
        let dir = tmp();
        let mut cfg = Config::default();
        cfg.strip_edit_preamble = false;
        let content = "The file /a.rs has been updated successfully. Here's the result of running `cat -n` on a snippet of the edited file:\n     1\thi";
        let json = format!(
            r#"{{"tool_name":"Edit","tool_result":{{"content":"{}"}}}}"#,
            content.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
        );
        // With strip disabled: Edit returns None (no other compression for Edit).
        assert!(compute_rewrite(&json, "Edit", &dir, &cfg).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_uses_lower_summarize_threshold() {
        let dir = tmp();
        let cfg = Config::default();
        // Build 200 benign lines: passes global 300 default but triggers
        // Read-specific 150 default (×2 benign = 300; so we need >300 for benign).
        // Use error markers to ensure non-benign path (threshold = base = 150).
        let mut lines = Vec::with_capacity(200);
        lines.push("error: synthetic".to_string());
        for i in 1..200 {
            lines.push(format!("line {}", i));
        }
        let content = lines.join("\n");
        let json = format!(
            r#"{{"tool_name":"Read","tool_result":{{"content":"{}"}}}}"#,
            content.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
        );
        let rewrite = compute_rewrite(&json, "Read", &dir, &cfg);
        assert!(
            rewrite.is_some(),
            "Read with 200 lines + error marker should trigger summarize at threshold 150"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_threshold_zero_falls_back_to_global() {
        let dir = tmp();
        let mut cfg = Config::default();
        cfg.read_summarize_threshold_lines = 0; // disable Read override
        cfg.summarize_threshold_lines = 1000;
        let mut lines = Vec::with_capacity(200);
        lines.push("error: x".to_string());
        for i in 1..200 {
            lines.push(format!("l{}", i));
        }
        let content = lines.join("\n");
        let json = format!(
            r#"{{"tool_name":"Read","tool_result":{{"content":"{}"}}}}"#,
            content.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
        );
        // 200 lines < 1000 global threshold → no rewrite.
        assert!(compute_rewrite(&json, "Read", &dir, &cfg).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn strip_edit_preamble_helper_handles_missing_anchor() {
        assert!(strip_edit_preamble("just a plain message").is_none());
        assert!(strip_edit_preamble("").is_none());
    }
}
