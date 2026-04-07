// PostToolUse hook reads the JSON tool result from stdin and pipes it
// to `squeez track-result <tool>`. We extract artifacts (file paths,
// errors) and feed them into the SessionContext so future Bash calls
// can dedup against already-seen state. We do NOT (and cannot) rewrite
// the model's view of the result — Claude Code's PostToolUse only allows
// observation. The win is cross-tool dedup in subsequent calls.

use std::io::Read;
use std::path::Path;

use crate::commands::wrap;
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
    if raw.trim().is_empty() {
        return 0;
    }

    // Parse minimal JSON via existing helpers (flat keys only).
    let file_path = extract_string_field(raw, "file_path");
    let pattern = extract_string_field(raw, "pattern");
    let path_arg = extract_string_field(raw, "path");
    let content = extract_content(raw);

    let mut ctx = SessionContext::load(sessions_dir);

    // Bump the call counter for context tracking; tool calls advance state too.
    ctx.next_call_n();

    // Files: from explicit fields + extracted from content
    let mut files: Vec<String> = Vec::new();
    if let Some(p) = file_path {
        files.push(p);
    }
    if let Some(p) = path_arg {
        files.push(p);
    }
    if let Some(content) = content.as_deref() {
        let trimmed: String = content.chars().take(MAX_CONTENT_BYTES).collect();
        let mut paths = wrap::extract_file_paths(&trimmed);
        files.append(&mut paths);
        let errors = wrap::extract_errors(&trimmed);
        if !errors.is_empty() {
            ctx.note_errors(&errors);
        }
    }
    if let Some(_p) = pattern {
        // No-op: pattern alone isn't a file fingerprint, but we may want
        // to log it as a tool_artifacts event in the session log.
    }

    if !files.is_empty() {
        ctx.note_files(&files);
    }

    // Track tokens consumed by this tool call (estimate from content length)
    if let Some(ref c) = content {
        let tokens = (c.len() / 4) as u64;
        if tokens > 0 {
            ctx.note_tool_tokens(tool, tokens);
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
        let d = std::env::temp_dir().join(format!(
            "squeez_track_result_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
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
}
