// PostToolUse rewrite hook — Claude Code v2.1.119+ added `updatedToolOutput`
// to PostToolUse for all tools, not just MCP. This command reads the
// PostToolUse JSON from stdin, checks the content against SessionContext for
// exact/fuzzy redundancy, and prints a hookSpecificOutput JSON blob when the
// content can be compressed. Exits 0 with no stdout when no rewrite is needed
// (Claude Code sees the original result unchanged).
//
// Called from hooks/posttooluse.sh for Read / Grep / Glob tool calls.
// Bash compression is already handled at PreToolUse via `squeez wrap`.

use std::io::Read;
use std::path::Path;

use crate::config::Config;
use crate::context;
use crate::session;

const MAX_CONTENT_BYTES: usize = 256 * 1024;

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
        emit_updated_output(&out);
    }
    0
}

/// Core logic: returns the rewritten content string if compression applies,
/// or `None` if the original should be kept as-is. Exposed for testing.
pub fn compute_rewrite(raw: &str, tool: &str, sessions_dir: &Path, cfg: &Config) -> Option<String> {
    if raw.trim().is_empty() {
        return None;
    }

    let content = extract_content(raw).filter(|c| !c.trim().is_empty())?;
    let content: String = content.chars().take(MAX_CONTENT_BYTES).collect();
    let lines: Vec<String> = content.lines().map(String::from).collect();

    if lines.is_empty() {
        return None;
    }

    let mut ctx = context::cache::SessionContext::load(sessions_dir);

    // Redundancy check: if we've seen this content before, replace with a note.
    if cfg.redundancy_cache_enabled {
        if let Some(hit) = context::redundancy::check(&ctx, &lines) {
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

    // Summarize fallback for very large outputs.
    let rewritten = if context::summarize::should_apply(&lines, cfg) {
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

fn emit_updated_output(content: &str) {
    println!(
        r#"{{"hookSpecificOutput":{{"hookEventName":"PostToolUse","updatedToolOutput":"{}"}}}}"#,
        json_escape(content)
    );
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
            if let Some(end) = stripped.find('"') {
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
}
