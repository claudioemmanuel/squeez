//! squeez MCP server — exposes session memory as a JSON-RPC 2.0 tool surface
//! over stdin/stdout, following the Model Context Protocol stdio transport.
//!
//! Hand-rolled, no `mcp-server` / `fastmcp` dependency — a stdin/stdout loop
//! with no upstream protocol library. Keeps squeez's `libc`-only constraint intact.
//!
//! Wire format: newline-delimited JSON-RPC 2.0 (one request per line, one
//! response per line). All tools are read-only. Tool names are namespaced
//! `squeez_*` so they don't collide with other MCP servers in the same
//! Claude Code session.

use std::io::{self, BufRead, Write};

use crate::commands::protocol;
use crate::context::cache::SessionContext;
use crate::json_util::escape_str;
use crate::{memory, session};

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "squeez";

/// Entry point for `squeez mcp`. Reads JSON-RPC requests from stdin (one per
/// line), writes responses to stdout, exits cleanly on EOF.
pub fn run() -> i32 {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut handle = stdin.lock();
    let mut input = String::new();

    loop {
        input.clear();
        match handle.read_line(&mut input) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(_) => break,
        }
        let line = input.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(resp) = handle_request(line) {
            let _ = writeln!(out, "{}", resp);
            let _ = out.flush();
        }
    }
    0
}

// ── Request dispatch ──────────────────────────────────────────────────────

/// Handle one JSON-RPC request line. Returns `None` for notifications
/// (which must not produce a response per JSON-RPC 2.0 spec).
pub fn handle_request(line: &str) -> Option<String> {
    let id = extract_id_raw(line);
    let method = extract_method(line);

    // Notifications have no `id` — silent.
    if id.is_none() {
        return None;
    }
    let id = id.unwrap();

    match method.as_deref() {
        Some("initialize") => Some(initialize_response(&id)),
        Some("tools/list") => Some(tools_list_response(&id)),
        Some("tools/call") => Some(tools_call_response(&id, line)),
        Some("ping") => Some(empty_result_response(&id)),
        Some(other) => Some(error_response(
            &id,
            -32601,
            &format!("method not found: {}", other),
        )),
        None => Some(error_response(&id, -32600, "invalid request")),
    }
}

// ── Response builders ─────────────────────────────────────────────────────

fn initialize_response(id: &str) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{{\
\"protocolVersion\":\"{}\",\
\"capabilities\":{{\"tools\":{{\"listChanged\":false}}}},\
\"serverInfo\":{{\"name\":\"{}\",\"version\":\"{}\"}}}}}}",
        id,
        PROTOCOL_VERSION,
        SERVER_NAME,
        env!("CARGO_PKG_VERSION"),
    )
}

fn empty_result_response(id: &str) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{{}}}}", id)
}

fn error_response(id: &str, code: i32, msg: &str) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{},\"error\":{{\"code\":{},\"message\":\"{}\"}}}}",
        id,
        code,
        escape_str(msg)
    )
}

fn text_result_response(id: &str, text: &str) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{{\"content\":[{{\"type\":\"text\",\"text\":\"{}\"}}]}}}}",
        id,
        escape_str(text)
    )
}

// ── Tool registry ─────────────────────────────────────────────────────────

/// Tool name → human-readable description, used both by `tools/list` and by
/// the `tools/call` dispatcher. Order matters: it's the display order in
/// `tools/list`. Schemas are inlined into the JSON because they're tiny.
const TOOLS: &[(&str, &str, &str)] = &[
    (
        "squeez_recent_calls",
        "List the most recent bash invocations squeez has compressed in this session, with output hash and length. Use to check whether you've already run a similar command before re-running it.",
        "{\"type\":\"object\",\"properties\":{\"n\":{\"type\":\"integer\",\"description\":\"max calls to return (default 10)\"}}}",
    ),
    (
        "squeez_seen_files",
        "List the files this session has touched via Read or via paths extracted from bash output, with the call number where each was last seen.",
        "{\"type\":\"object\",\"properties\":{\"limit\":{\"type\":\"integer\",\"description\":\"max files to return (default 20)\"}}}",
    ),
    (
        "squeez_seen_errors",
        "List the count of distinct error fingerprints squeez has observed this session. Errors are normalized (digits, paths, hex collapsed) so reruns don't double-count.",
        "{\"type\":\"object\",\"properties\":{\"limit\":{\"type\":\"integer\",\"description\":\"max errors to return (default 10)\"}}}",
    ),
    (
        "squeez_session_summary",
        "Token accounting and call counts for the current session: tokens by tool category (Bash/Read/Other), total calls, files seen, errors seen, git refs seen.",
        "{\"type\":\"object\",\"properties\":{}}",
    ),
    (
        "squeez_prior_summaries",
        "Read the most recent finalized prior-session summaries from memory/summaries.jsonl. Includes files touched, files committed, test results, errors resolved, and git activity per session.",
        "{\"type\":\"object\",\"properties\":{\"n\":{\"type\":\"integer\",\"description\":\"max sessions to return (default 5)\"}}}",
    ),
    (
        "squeez_protocol",
        "Returns the squeez memory protocol + output marker spec. Read this once per session to understand the headers and `[squeez: ...]` markers in compressed output.",
        "{\"type\":\"object\",\"properties\":{}}",
    ),
];

fn tools_list_response(id: &str) -> String {
    let mut tools_json = String::from("[");
    for (i, (name, desc, schema)) in TOOLS.iter().enumerate() {
        if i > 0 {
            tools_json.push(',');
        }
        tools_json.push_str(&format!(
            "{{\"name\":\"{}\",\"description\":\"{}\",\"inputSchema\":{}}}",
            name,
            escape_str(desc),
            schema
        ));
    }
    tools_json.push(']');
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{{\"tools\":{}}}}}",
        id, tools_json
    )
}

fn tools_call_response(id: &str, line: &str) -> String {
    let name = match crate::json_util::extract_str(line, "name") {
        Some(n) => n,
        None => return error_response(id, -32602, "missing tool name"),
    };
    // All tools take optional integer params (`n` or `limit`); extract both.
    let n = crate::json_util::extract_u64(line, "n").map(|v| v as usize);
    let limit = crate::json_util::extract_u64(line, "limit").map(|v| v as usize);

    let text = match name.as_str() {
        "squeez_recent_calls" => tool_recent_calls(n.unwrap_or(10)),
        "squeez_seen_files" => tool_seen_files(limit.unwrap_or(20)),
        "squeez_seen_errors" => tool_seen_errors(limit.unwrap_or(10)),
        "squeez_session_summary" => tool_session_summary(),
        "squeez_prior_summaries" => tool_prior_summaries(n.unwrap_or(5)),
        "squeez_protocol" => protocol::full_payload(),
        other => return error_response(id, -32602, &format!("unknown tool: {}", other)),
    };
    text_result_response(id, &text)
}

// ── Tool implementations ──────────────────────────────────────────────────

fn load_ctx() -> SessionContext {
    SessionContext::load(&session::sessions_dir())
}

fn tool_recent_calls(n: usize) -> String {
    let ctx = load_ctx();
    if ctx.call_log.is_empty() {
        return "(no calls recorded yet in this session)".to_string();
    }
    let take = n.min(ctx.call_log.len());
    let start = ctx.call_log.len() - take;
    let mut out = format!(
        "session={} call_counter={} showing last {} of {} calls\n",
        ctx.session_file,
        ctx.call_counter,
        take,
        ctx.call_log.len()
    );
    for entry in &ctx.call_log[start..] {
        out.push_str(&format!(
            "#{:>4}  {}  {} bytes  {}\n",
            entry.call_n, entry.short_hash, entry.output_len, entry.cmd_short
        ));
    }
    out
}

fn tool_seen_files(limit: usize) -> String {
    let ctx = load_ctx();
    if ctx.seen_files.is_empty() {
        return "(no files seen yet in this session)".to_string();
    }
    // Sort by recency (highest last_seen_call first), then take `limit`.
    let mut files = ctx.seen_files.clone();
    files.sort_by(|a, b| b.last_seen_call.cmp(&a.last_seen_call));
    let take = limit.min(files.len());
    let mut out = format!("seen_files total={} showing={}\n", files.len(), take);
    for f in files.iter().take(take) {
        out.push_str(&format!("call#{:>4}  {}\n", f.last_seen_call, f.path));
    }
    out
}

fn tool_seen_errors(limit: usize) -> String {
    let ctx = load_ctx();
    if ctx.seen_errors.is_empty() {
        return "(no errors seen yet in this session)".to_string();
    }
    let take = limit.min(ctx.seen_errors.len());
    let mut out = format!(
        "seen_errors distinct={} showing={}\n",
        ctx.seen_errors.len(),
        take
    );
    out.push_str("(values are FNV-1a-64 fingerprints of normalized error strings; \
identity, not content — squeez stores hashes only)\n");
    for fp in ctx.seen_errors.iter().take(take) {
        out.push_str(&format!("  {:016x}\n", fp));
    }
    out
}

fn tool_session_summary() -> String {
    let ctx = load_ctx();
    let curr = session::CurrentSession::load(&session::sessions_dir());
    let mut out = String::from("squeez session summary\n");
    out.push_str(&format!("session_file:    {}\n", ctx.session_file));
    out.push_str(&format!("call_counter:    {}\n", ctx.call_counter));
    out.push_str(&format!("calls_logged:    {}\n", ctx.call_log.len()));
    out.push_str(&format!("seen_files:      {}\n", ctx.seen_files.len()));
    out.push_str(&format!("seen_errors:     {}\n", ctx.seen_errors.len()));
    out.push_str(&format!("seen_git_refs:   {}\n", ctx.seen_git_refs.len()));
    out.push_str(&format!("tokens_bash:     {}\n", ctx.tokens_bash));
    out.push_str(&format!("tokens_read:     {}\n", ctx.tokens_read));
    out.push_str(&format!("tokens_other:    {}\n", ctx.tokens_other));
    if let Some(c) = curr {
        out.push_str(&format!("session_total:   {} tokens\n", c.total_tokens));
        out.push_str(&format!("started_unix:    {}\n", c.start_ts));
    }
    out
}

fn tool_prior_summaries(n: usize) -> String {
    let memory_dir = session::memory_dir();
    let summaries = memory::read_last_n(&memory_dir, n);
    if summaries.is_empty() {
        return "(no prior session summaries on disk yet)".to_string();
    }
    let mut out = format!("showing {} prior session(s)\n", summaries.len());
    for s in &summaries {
        out.push_str(&format!(
            "─ {} ({} min)  files:{}  commits:{}  tests:{}  errors_resolved:{}\n",
            s.date,
            s.duration_min,
            s.files_touched.len(),
            s.git_events.len(),
            if s.test_summary.is_empty() {
                "—"
            } else {
                &s.test_summary
            },
            s.errors_resolved.len(),
        ));
        if !s.files_committed.is_empty() {
            out.push_str(&format!(
                "    committed: {}\n",
                s.files_committed.join(", ")
            ));
        }
    }
    out
}

// ── JSON helpers (raw `id` extraction) ────────────────────────────────────

/// Extract the raw `"id"` value from a JSON-RPC request — number, string,
/// or `null` — preserved verbatim so we can echo it back in the response.
/// Returns `None` if the request has no `id` (i.e. it's a notification).
fn extract_id_raw(json: &str) -> Option<String> {
    let pat = "\"id\":";
    let start = json.find(pat)? + pat.len();
    let s = json[start..].trim_start();
    if s.is_empty() {
        return None;
    }
    // String value
    if s.starts_with('"') {
        let rest = &s[1..];
        let end = rest.find('"')?;
        return Some(format!("\"{}\"", &rest[..end]));
    }
    // Number or null/true/false — read until comma or closing brace
    let end = s
        .find(|c: char| c == ',' || c == '}')
        .unwrap_or(s.len());
    let raw = s[..end].trim();
    if raw.is_empty() {
        None
    } else {
        Some(raw.to_string())
    }
}

fn extract_method(json: &str) -> Option<String> {
    crate::json_util::extract_str(json, "method")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_id_raw_handles_number() {
        assert_eq!(
            extract_id_raw("{\"jsonrpc\":\"2.0\",\"id\":42,\"method\":\"x\"}"),
            Some("42".to_string())
        );
    }

    #[test]
    fn extract_id_raw_handles_string() {
        assert_eq!(
            extract_id_raw("{\"jsonrpc\":\"2.0\",\"id\":\"abc\",\"method\":\"x\"}"),
            Some("\"abc\"".to_string())
        );
    }

    #[test]
    fn extract_id_raw_returns_none_for_notification() {
        // Notification: no `id` field at all
        assert_eq!(
            extract_id_raw("{\"jsonrpc\":\"2.0\",\"method\":\"notify\"}"),
            None
        );
    }

    #[test]
    fn handle_initialize_returns_protocol_version() {
        let req = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}";
        let resp = handle_request(req).expect("must respond");
        assert!(resp.contains("\"protocolVersion\":\"2024-11-05\""));
        assert!(resp.contains("\"name\":\"squeez\""));
        assert!(resp.contains("\"id\":1"));
    }

    #[test]
    fn handle_tools_list_returns_six_tools() {
        let req = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}";
        let resp = handle_request(req).expect("must respond");
        // All six tool names appear in the response.
        for name in [
            "squeez_recent_calls",
            "squeez_seen_files",
            "squeez_seen_errors",
            "squeez_session_summary",
            "squeez_prior_summaries",
            "squeez_protocol",
        ] {
            assert!(resp.contains(name), "missing tool {}", name);
        }
    }

    #[test]
    fn handle_unknown_method_returns_error() {
        let req = "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"bogus\"}";
        let resp = handle_request(req).expect("must respond");
        assert!(resp.contains("\"error\""));
        assert!(resp.contains("-32601"));
    }

    #[test]
    fn handle_notification_returns_none() {
        // No `id` field → no response.
        let req = "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}";
        assert!(handle_request(req).is_none());
    }

    #[test]
    fn handle_tools_call_protocol_returns_payload() {
        let req = "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\
\"params\":{\"name\":\"squeez_protocol\",\"arguments\":{}}}";
        let resp = handle_request(req).expect("must respond");
        assert!(resp.contains("\"content\""));
        assert!(resp.contains("squeez memory protocol"));
        assert!(resp.contains("squeez output markers"));
    }

    #[test]
    fn handle_tools_call_unknown_returns_error() {
        let req = "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"tools/call\",\
\"params\":{\"name\":\"bogus_tool\",\"arguments\":{}}}";
        let resp = handle_request(req).expect("must respond");
        assert!(resp.contains("\"error\""));
        assert!(resp.contains("unknown tool"));
    }

    #[test]
    fn ping_returns_empty_result() {
        let req = "{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"ping\"}";
        let resp = handle_request(req).expect("must respond");
        assert!(resp.contains("\"result\":{}"));
    }
}
