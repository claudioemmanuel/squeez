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
    // SubagentStop can't rewrite the sub-agent's returned message (the platform
    // has no `updatedToolOutput` for Stop-family events — verified against the
    // hooks reference), so token-saving condensation is impossible here: the
    // message flows to the parent unchanged. The one supported lever is
    // `additionalContext` — used to append a one-shot behavioral nudge steering
    // future sub-agents to return a small summary + file path instead of a wall
    // of text. Never touches the message itself.
    let out = render_for_test(raw, tool, sessions_dir, cfg);
    if !out.is_empty() {
        println!("{}", out);
    }
    0
}

/// The full PostToolUse decision, returned rather than printed.
///
/// `run_with` is a thin printer over this so the emitted JSON can be asserted
/// on directly; an empty string means "emit nothing and leave the tool result
/// untouched".
pub fn render_for_test(raw: &str, tool: &str, sessions_dir: &Path, cfg: &Config) -> String {
    if tool == "SubagentStop" {
        return match subagent_advisory(raw, cfg) {
            Some(msg) => render_additional_context("SubagentStop", &msg),
            None => String::new(),
        };
    }
    let rewrite = compute_rewrite(raw, tool, sessions_dir, cfg);
    // Economy warnings must not depend on a Bash command happening to run.
    // `burst_warning`/`agent_cost_warning` were reachable ONLY from wrap.rs, so
    // through the whole 2026-08-21 fan-out — which used Agent, WebFetch and
    // WebSearch, never Bash — squeez had the right numbers and no way to say
    // them. PostToolUse fires for every tool, so they ride here too.
    let notices = economy_notices(sessions_dir, cfg);
    match rewrite {
        // One hookSpecificOutput per hook invocation: when output is being
        // rewritten anyway, the notices ride inside it rather than as a second
        // JSON object the host would ignore.
        Some(out) => {
            let body = if notices.is_empty() {
                out
            } else {
                format!("{}\n{}", notices.join("\n"), out)
            };
            render_updated_output(raw, tool, &body)
        }
        None if !notices.is_empty() => {
            render_additional_context("PostToolUse", &notices.join("\n"))
        }
        None => String::new(),
    }
}

/// Pending economy warnings for this call, deduped so a standing condition is
/// stated once rather than on every tool result.
fn economy_notices(sessions_dir: &Path, cfg: &Config) -> Vec<String> {
    use crate::context::cache::SessionContext;
    use crate::economy::agent_tracker;
    let mut out = Vec::new();
    SessionContext::update(sessions_dir, |ctx| {
        let call_n = ctx.call_counter;
        if let Some(w) = agent_tracker::burst_warning(ctx, cfg) {
            if crate::context::cache::dedup_header_tag(
                &mut ctx.last_burst_tag,
                &mut ctx.last_burst_tag_call_n,
                &w,
                call_n,
            ) {
                out.push(w);
            }
        }
        if let Some(w) = agent_tracker::agent_cost_warning(ctx, cfg) {
            if crate::context::cache::dedup_header_tag(
                &mut ctx.last_agent_tag,
                &mut ctx.last_agent_tag_call_n,
                &w,
                call_n,
            ) {
                out.push(w);
            }
        }
    });
    out
}

/// Behavioral advisory for an oversized sub-agent return. `None` unless the
/// returned content exceeds `subagent_result_warn_tokens` (0 disables). The
/// message rides in `additionalContext`, not the sub-agent output — it cannot
/// shrink this return, only discourage the next one from being huge.
pub fn subagent_advisory(raw: &str, cfg: &Config) -> Option<String> {
    if cfg.subagent_result_warn_tokens == 0 {
        return None;
    }
    let content = extract_content(raw).filter(|c| !c.trim().is_empty())?;
    let tokens = content.len() / 4;
    if tokens <= cfg.subagent_result_warn_tokens {
        return None;
    }
    Some(format!(
        "[squeez: sub-agent returned ~{}K tokens into the parent context, which \
         rides in the prefix every remaining turn — next time have the sub-agent \
         write detail to a file and return a <=2K summary + path]",
        (tokens / 1000).max(1)
    ))
}

/// Render an `additionalContext` hookSpecificOutput payload for a Stop-family
/// event (SubagentStop/Stop). Unlike `updatedToolOutput`, this appends context
/// rather than replacing tool output.
fn render_additional_context(event: &str, msg: &str) -> String {
    format!(
        r#"{{"hookSpecificOutput":{{"hookEventName":"{}","additionalContext":"{}"}}}}"#,
        event,
        json_escape(msg)
    )
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

    // Session-long read dedup. `redundancy::check` below only reaches back
    // `recent_window` (16) calls, but re-reading an unchanged file recurs far
    // past that — the same reason skill injections need their own store.
    // Keyed by path AND content hash, exact-match only: a long-distance fuzzy
    // match is precisely the A2 failure mode. The floor check inside
    // `read_dedup_lookup` keeps prior-session and pre-compaction reads out.
    let read_dedup_key = if cfg.read_dedup_session_long
        && cfg.redundancy_cache_enabled
        && tool == "Read"
        && !edited_since_seen
    {
        extract_string_field(raw, "file_path")
            .map(|p| unescape(&p))
            .map(|p| (p, crate::context::hash::fnv1a_64(content.as_bytes())))
    } else {
        None
    };
    if let Some((ref path, fp)) = read_dedup_key {
        if let Some(call_n) = ctx.read_dedup_lookup(path, fp) {
            let note = format!("[squeez: identical to Read #{call_n} of {path} — output omitted]");
            if marker_wins(&note, &content, cfg) {
                ctx.exact_dedup_hits += 1;
                ctx.save(sessions_dir);
                return Some(note);
            }
        }
    }

    // Redundancy check: if we've seen this content before, replace with a note.
    if cfg.redundancy_cache_enabled {
        if let Some(hit) = context::redundancy::check(&ctx, &lines) {
            let fuzzy_blocked = hit.similarity.is_some() && (edited_since_seen || is_mcp);
            if !fuzzy_blocked {
                // Name the call that matched, not just the current tool: for
                // Read/Grep/Glob the label carries the target, so the model can
                // tell "this file holds the same bytes as that one" apart from
                // "this is the same file". Falls back to the tool name for
                // labels that carry no target (bash, MCP, legacy entries).
                let target = hit
                    .label
                    .strip_prefix(tool)
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(|t| format!(" ({})", t))
                    .unwrap_or_default();
                let note = match hit.similarity {
                    None => format!(
                        "[squeez: identical to {} #{}{} — output omitted]",
                        tool, hit.call_n, target
                    ),
                    Some(j) => format!(
                        "[squeez: ~{}% similar to {} #{}{} — re-read if needed]",
                        (j * 100.0).round() as u32,
                        tool,
                        hit.call_n,
                        target
                    ),
                };
                if marker_wins(&note, &content, cfg) {
                    ctx.exact_dedup_hits += 1;
                    ctx.save(sessions_dir);
                    return Some(note);
                }
            }
        }
    }

    // Large benign outputs get summarized (Skill is handled earlier via the
    // session-long dedup store and never reaches here). MCP results and Read
    // are dedup-only, never summarized: both carry structured, non-repetitive
    // payloads (JSON, DOM snapshots, source files) the log-oriented summarizer
    // would corrupt. Read shares MCP's failure mode: `is_benign` does raw
    // substring matching for "error"/"failed"/"panic", which fires on ordinary
    // source code that merely mentions those words, collapsing the relaxed
    // threshold and handing the summarizer a source file — it then keeps a
    // tail slice plus incidental keyword hits and drops the rest, which is
    // exactly the content needed to reason about or Edit the file.
    let is_structured_payload = is_mcp || tool == "Read";
    let rewritten =
        if !is_structured_payload && context::summarize::should_apply_for_tool(&lines, cfg, tool) {
            let summary = context::summarize::apply(lines.clone(), tool);
            if summary.len() < lines.len() {
                Some(summary.join("\n"))
            } else {
                None
            }
        } else {
            None
        };

    // Record content so future calls can dedup against it. The read store
    // reuses the call_n minted here, so one Read consumes one call number.
    if cfg.redundancy_cache_enabled {
        let call_n = context::redundancy::record(&mut ctx, &dedup_label(tool, raw), &lines);
        if let Some((path, fp)) = read_dedup_key {
            ctx.read_dedup_record(&path, fp, call_n);
        }
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
        let total_lines =
            crate::json_util::extract_u64(raw, "totalLines").unwrap_or(num_lines as u64);
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
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Whether replacing `original` with `marker` is worth doing.
///
/// `wrap.rs` gates its header on `net_win_min_tokens`, but the dedup markers
/// on this path never had a gate: a 3-line file re-read was replaced by a
/// marker that embeds the full path and can cost more than the content it
/// "saved". A tool that spends tokens to save tokens has to check.
fn marker_wins(marker: &str, original: &str, cfg: &Config) -> bool {
    let before = crate::tokens::estimate(original);
    let after = crate::tokens::estimate(marker);
    before.saturating_sub(after) >= cfg.net_win_min_tokens
}

/// Label recorded in the call log for a tool result.
///
/// For Read/Glob/Grep the output alone does not identify the call: two files
/// can hold byte-identical content, and two greps can return the same hit set.
/// Appending the target makes the dedup marker unambiguous. `CallEntry` keeps
/// only the first 40 chars, so a long path contributes its tail — the part
/// that identifies it — rather than a shared `/Users/...` prefix.
fn dedup_label(tool: &str, raw: &str) -> String {
    let target = match tool {
        "Read" | "Glob" => extract_string_field(raw, "file_path").map(|p| unescape(&p)),
        "Grep" => extract_string_field(raw, "pattern").map(|p| unescape(&p)),
        _ => None,
    };
    match target {
        Some(t) if !t.is_empty() => format!("{} {}", tool, truncate_target(&t)),
        _ => tool.to_string(),
    }
}

/// Keep the identifying tail of a target within the 40-char `cmd_short` cap,
/// cutting at a path separator so the result reads as a path and not as a
/// word sliced in half.
fn truncate_target(t: &str) -> String {
    const KEEP: usize = 40 - 6; // cap, minus "Read " + the ellipsis
    if t.chars().count() <= KEEP {
        return t.to_string();
    }
    let skip = t.chars().count() - KEEP;
    let tail: String = t.chars().skip(skip).collect();
    match tail.find('/') {
        Some(i) => format!("…{}", &tail[i..]),
        None => format!("…{}", tail),
    }
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
        assert!(
            out.contains(r#""updatedToolOutput":{"type":"text","file":{"filePath":"/tmp/a.rs""#)
        );
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
        assert_eq!(
            compute_rewrite(&json, "mcp__chrome-devtools__take_screenshot", &dir, &cfg),
            None
        );
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
        let original: Vec<String> = (0..40)
            .map(|i| format!("line {} alpha beta gamma", i))
            .collect();
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

        let original: Vec<String> = (0..40)
            .map(|i| format!("line {} alpha beta gamma", i))
            .collect();
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
            rewrite
                .as_deref()
                .map(|r| r.contains("similar"))
                .unwrap_or(false),
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
            content
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
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
            content
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
        );
        // With strip disabled: Edit returns None (no other compression for Edit).
        assert!(compute_rewrite(&json, "Edit", &dir, &cfg).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_is_never_summarized_even_with_error_marker() {
        // Read is a structured payload (source/config/data), not log output —
        // see the is_structured_payload comment above. A stray "error:" line
        // (e.g. a comment, a string literal) must not trigger the dense
        // log summarizer regardless of line count or threshold.
        let dir = tmp();
        let cfg = Config::default();
        let mut lines = Vec::with_capacity(200);
        lines.push("error: synthetic".to_string());
        for i in 1..200 {
            lines.push(format!("line {}", i));
        }
        let content = lines.join("\n");
        let json = format!(
            r#"{{"tool_name":"Read","tool_result":{{"content":"{}"}}}}"#,
            content
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
        );
        let rewrite = compute_rewrite(&json, "Read", &dir, &cfg);
        assert!(
            rewrite.is_none(),
            "Read must never be replaced by the dense summarizer"
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
            content
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
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

    #[test]
    fn subagent_advisory_fires_above_threshold_only() {
        let cfg = Config::default(); // subagent_result_warn_tokens = 3000
                                     // ~5K tokens (20K chars) > 3K → advisory.
        let big = "word ".repeat(4_000);
        let json = format!(
            r#"{{"tool_name":"SubagentStop","tool_result":{{"content":"{}"}}}}"#,
            big.trim()
        );
        let a = subagent_advisory(&json, &cfg).expect("advisory expected");
        assert!(a.contains("sub-agent returned"));
        assert!(a.contains("summary + path"));
        // Small return → no advisory.
        let small = r#"{"tool_name":"SubagentStop","tool_result":{"content":"done: no issues"}}"#;
        assert!(subagent_advisory(small, &cfg).is_none());
    }

    #[test]
    fn subagent_advisory_disabled_when_threshold_zero() {
        let mut cfg = Config::default();
        cfg.subagent_result_warn_tokens = 0;
        let big = "word ".repeat(4_000);
        let json = format!(
            r#"{{"tool_name":"SubagentStop","tool_result":{{"content":"{}"}}}}"#,
            big.trim()
        );
        assert!(subagent_advisory(&json, &cfg).is_none());
    }

    #[test]
    fn subagent_stop_emits_additional_context_not_updated_output() {
        // SubagentStop uses additionalContext (append), never updatedToolOutput
        // (replace) — the sub-agent message can't be rewritten.
        let big = "word ".repeat(4_000);
        let msg = subagent_advisory(
            &format!(
                r#"{{"tool_name":"SubagentStop","tool_result":{{"content":"{}"}}}}"#,
                big.trim()
            ),
            &Config::default(),
        )
        .unwrap();
        let out = render_additional_context("SubagentStop", &msg);
        assert!(out.contains(r#""hookEventName":"SubagentStop""#));
        assert!(out.contains(r#""additionalContext""#));
        assert!(!out.contains("updatedToolOutput"));
    }
}
