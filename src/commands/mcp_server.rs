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
use crate::session;

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
    (
        "squeez_seen_error_details",
        "List error snippets (first 128 chars) for unique errors seen this session. More informative than squeez_seen_errors which returns only fingerprints.",
        "{\"type\":\"object\",\"properties\":{\"limit\":{\"type\":\"integer\",\"description\":\"max errors to return (default 10)\"}}}",
    ),
    (
        "squeez_search_history",
        "Search prior session summaries for a keyword (case-insensitive substring). Returns sessions where the query appears in files, errors, git events, or structured summary fields. Scans up to 200 sessions.",
        "{\"type\":\"object\",\"properties\":{\"query\":{\"type\":\"string\",\"description\":\"search term\"},\"limit\":{\"type\":\"integer\",\"description\":\"max results (default 10)\"}},\"required\":[\"query\"]}",
    ),
    (
        "squeez_file_history",
        "Show prior sessions where a given file path was touched or committed. Returns session dates and token savings.",
        "{\"type\":\"object\",\"properties\":{\"path\":{\"type\":\"string\",\"description\":\"file path substring to match\"},\"limit\":{\"type\":\"integer\",\"description\":\"max results (default 10)\"}},\"required\":[\"path\"]}",
    ),
    (
        "squeez_session_detail",
        "Return a structured view of a specific prior session by date (YYYY-MM-DD). Shows total events, files, errors, git events, and test results. Truncated to 2 KB if large.",
        "{\"type\":\"object\",\"properties\":{\"date\":{\"type\":\"string\",\"description\":\"session date YYYY-MM-DD\"}},\"required\":[\"date\"]}",
    ),
    (
        "squeez_session_stats",
        "Compression statistics for the current session: exact/fuzzy dedup hits, summarize triggers, intensity ultra calls, and token savings by handler category.",
        "{\"type\":\"object\",\"properties\":{}}",
    ),
    (
        "squeez_agent_costs",
        "Sub-agent usage tracking: number of Agent/Task tool spawns this session, estimated hidden context cost (~200K tokens per spawn), and per-call breakdown.",
        "{\"type\":\"object\",\"properties\":{}}",
    ),
    (
        "squeez_session_efficiency",
        "Session efficiency scoring: compression ratio, tool choice efficiency (direct vs agent), context reuse rate, budget conservation. Scores in basis points (0-10000 = 0-100%).",
        "{\"type\":\"object\",\"properties\":{}}",
    ),
    (
        "squeez_context_pressure",
        "Current context pressure: budget used %, calls remaining, tokens_saved this session, and an actionable recommendation (ok / compact_soon / use_state_first). Call this to decide whether to /compact or save state and /clear.",
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
    // String params for search/history/detail tools.
    let query = crate::json_util::extract_str(line, "query").unwrap_or_default();
    let path_arg = crate::json_util::extract_str(line, "path").unwrap_or_default();
    let date_arg = crate::json_util::extract_str(line, "date").unwrap_or_default();

    let cfg = crate::config::Config::load();
    let text = match name.as_str() {
        "squeez_recent_calls" => tool_recent_calls(n.unwrap_or(cfg.mcp_recent_calls_default)),
        "squeez_seen_files" => tool_seen_files(limit.unwrap_or(20)),
        "squeez_seen_errors" => tool_seen_errors(limit.unwrap_or(10)),
        "squeez_session_summary" => tool_session_summary(),
        "squeez_prior_summaries" => tool_prior_summaries(n.unwrap_or(cfg.mcp_prior_summaries_default)),
        "squeez_protocol" => protocol::full_payload(),
        "squeez_seen_error_details" => tool_seen_error_details(limit.unwrap_or(10)),
        "squeez_search_history" => tool_search_history(&query, limit.unwrap_or(10)),
        "squeez_file_history" => tool_file_history(&path_arg, limit.unwrap_or(10)),
        "squeez_session_detail" => tool_session_detail(&date_arg),
        "squeez_session_stats" => tool_session_stats(),
        "squeez_agent_costs" => tool_agent_costs(),
        "squeez_session_efficiency" => tool_session_efficiency(),
        "squeez_context_pressure" => tool_context_pressure(),
        other => return error_response(id, -32602, &format!("unknown tool: {}", other)),
    };
    text_result_response(id, &text)
}

// ── Tool implementations ──────────────────────────────────────────────────

use std::cell::RefCell;
use std::time::SystemTime;

struct CachedCtx {
    mtime: SystemTime,
    ctx: SessionContext,
}

thread_local! {
    static CTX_CACHE: RefCell<Option<CachedCtx>> = const { RefCell::new(None) };
}

fn load_ctx() -> SessionContext {
    let sessions = session::sessions_dir();
    let path = sessions.join("context.json");
    let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    CTX_CACHE.with(|cache| {
        // Check cache validity
        if let (Some(mt), Some(cached)) = (mtime, cache.borrow().as_ref()) {
            if cached.mtime == mt {
                return cached.ctx.clone();
            }
        }
        // Cache miss — load from disk
        let ctx = SessionContext::load(&sessions);
        if let Some(mt) = mtime {
            *cache.borrow_mut() = Some(CachedCtx { mtime: mt, ctx: ctx.clone() });
        }
        ctx
    })
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
    out.push_str(&format!("tokens_grep:     {}\n", ctx.tokens_grep));
    out.push_str(&format!("tokens_other:    {}\n", ctx.tokens_other));
    if ctx.reread_count > 0 {
        out.push_str(&format!("re-reads:        {} (same file accessed again after earlier read)\n", ctx.reread_count));
    }
    if let Some(c) = curr {
        out.push_str(&format!("session_total:   {} tokens\n", c.total_tokens));
        out.push_str(&format!("tokens_saved:    {} tokens\n", c.tokens_saved));
        let raw = c.total_tokens + c.tokens_saved;
        if raw > 0 {
            let ratio = c.tokens_saved * 100 / raw;
            out.push_str(&format!("compression:     {}%\n", ratio));
        }
        out.push_str(&format!("started_unix:    {}\n", c.start_ts));
    }
    out
}

fn tool_prior_summaries(n: usize) -> String {
    let summaries = read_rich_summaries(n);
    if summaries.is_empty() {
        return "(no prior session summaries on disk yet)".to_string();
    }
    let mut out = format!("showing {} prior session(s)\n", summaries.len());
    for s in &summaries {
        out.push_str(&format!(
            "─ {} ({} min)  files:{}  commits:{}  tests:{}  errors_resolved:{}  tokens_saved:{}\n",
            s.date,
            s.duration_min,
            s.files_touched.len(),
            s.git_events.len(),
            if s.test_summary.is_empty() { "—" } else { &s.test_summary },
            s.errors_resolved.len(),
            s.tokens_saved,
        ));
        if !s.files_committed.is_empty() {
            out.push_str(&format!("    committed: {}\n", s.files_committed.join(", ")));
        }
        if !s.investigated.is_empty() {
            let sample: Vec<&String> = s.investigated.iter().take(3).collect();
            out.push_str(&format!("    investigated: {}", sample.iter().map(|x| x.as_str()).collect::<Vec<_>>().join(", ")));
            if s.investigated.len() > 3 { out.push_str(&format!(" (+{})", s.investigated.len() - 3)); }
            out.push('\n');
        }
        if !s.learned.is_empty() {
            out.push_str(&format!("    learned: {}\n", s.learned.join("; ")));
        }
        if !s.completed.is_empty() {
            out.push_str(&format!("    completed: {}\n", s.completed.join("; ")));
        }
        if !s.next_steps.is_empty() {
            out.push_str(&format!("    next_steps: {}\n", s.next_steps.join("; ")));
        }
    }
    out
}

// ── Rich summary reader (includes new optional structured fields) ─────────

/// Reads up to `n` most-recent summaries from `summaries.jsonl`, including
/// optional new structured fields added by the memory improvements phase
/// (`investigated`, `learned`, `completed`, `next_steps`). Old entries without
/// these fields produce empty Vecs — fully backwards-compatible.
struct RichSummary {
    date: String,
    duration_min: u64,
    files_touched: Vec<String>,
    files_committed: Vec<String>,
    test_summary: String,
    errors_resolved: Vec<String>,
    git_events: Vec<String>,
    ts: u64,
    tokens_saved: u64,
    // Phase-1 structured fields (empty for legacy JSONL entries)
    investigated: Vec<String>,
    learned: Vec<String>,
    completed: Vec<String>,
    next_steps: Vec<String>,
}

fn read_rich_summaries(n: usize) -> Vec<RichSummary> {
    let memory_dir = session::memory_dir();
    let path = memory_dir.join("summaries.jsonl");
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if size > crate::memory::MAX_FILE_BYTES {
        return Vec::new();
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let mut summaries: Vec<RichSummary> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let date = crate::json_util::extract_str(l, "date")?;
            let ts = crate::json_util::extract_u64(l, "ts").unwrap_or(0);
            Some(RichSummary {
                date,
                duration_min: crate::json_util::extract_u64(l, "duration_min").unwrap_or(0),
                files_touched: crate::json_util::extract_str_array(l, "files_touched"),
                files_committed: crate::json_util::extract_str_array(l, "files_committed"),
                test_summary: crate::json_util::extract_str(l, "test_summary").unwrap_or_default(),
                errors_resolved: crate::json_util::extract_str_array(l, "errors_resolved"),
                git_events: crate::json_util::extract_str_array(l, "git_events"),
                ts,
                tokens_saved: crate::json_util::extract_u64(l, "tokens_saved").unwrap_or(0),
                investigated: crate::json_util::extract_str_array(l, "investigated"),
                learned: crate::json_util::extract_str_array(l, "learned"),
                completed: crate::json_util::extract_str_array(l, "completed"),
                next_steps: crate::json_util::extract_str_array(l, "next_steps"),
            })
        })
        .collect();
    summaries.sort_by(|a, b| b.ts.cmp(&a.ts));
    summaries.truncate(n);
    summaries
}

// ── New tool implementations (phases 2, 3, 6) ────────────────────────────

/// Phase 2: error snippets — returns stored (fingerprint, snippet) pairs.
/// Falls back to fingerprint-only display if Worker 1 hasn't added snippet
/// storage yet (graceful degradation via direct context.json read).
fn tool_seen_error_details(limit: usize) -> String {
    let path = session::sessions_dir().join("context.json");
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > crate::memory::MAX_FILE_BYTES {
        return "(context.json too large)".to_string();
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return "(no context.json — no active session or snippets not yet recorded)".to_string(),
    };
    // Worker 1 adds parallel arrays: error_snippets_fp + error_snippets_text.
    let fps = crate::json_util::extract_u64_array(&content, "error_snippets_fp");
    let texts = crate::json_util::extract_str_array(&content, "error_snippets_text");
    if fps.is_empty() && texts.is_empty() {
        // Graceful fallback: fingerprints-only from seen_errors.
        let seen = crate::json_util::extract_u64_array(&content, "seen_errors");
        if seen.is_empty() {
            return "(no errors seen yet in this session)".to_string();
        }
        let take = limit.min(seen.len());
        let mut out = format!(
            "seen_errors distinct={} showing={} (fingerprints only — snippets not yet recorded)\n",
            seen.len(), take
        );
        for fp in seen.iter().take(take) {
            out.push_str(&format!("  {:016x}\n", fp));
        }
        return out;
    }
    let count = fps.len().max(texts.len());
    let take = limit.min(count);
    let mut out = format!("error_snippets total={} showing={}\n", count, take);
    for i in 0..take {
        let fp = fps.get(i).copied().unwrap_or(0);
        let text = texts.get(i).cloned().unwrap_or_default();
        out.push_str(&format!("  {:016x}  {}\n", fp, escape_str(&text)));
    }
    out
}

/// Phase 3: cross-session keyword search across summaries.jsonl fields.
fn tool_search_history(query: &str, limit: usize) -> String {
    if query.is_empty() {
        return "(query is empty — provide a search term)".to_string();
    }
    let path = session::memory_dir().join("summaries.jsonl");
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > crate::memory::MAX_FILE_BYTES {
        return "(summaries.jsonl too large — run squeez prune)".to_string();
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return "(no session history on disk)".to_string(),
    };
    let lq = query.to_lowercase();
    let mut results: Vec<String> = Vec::new();
    let mut scanned: usize = 0;
    // Scan reverse-chronologically; cap at 200 sessions.
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    'outer: for line in lines.iter().rev().take(200) {
        scanned += 1;
        let date = crate::json_util::extract_str(line, "date").unwrap_or_default();
        // Check each field family for a substring match.
        let string_fields: &[&str] = &[
            "files_touched", "files_committed", "errors_resolved",
            "git_events", "investigated", "learned", "completed", "next_steps",
        ];
        for &field in string_fields {
            let arr = crate::json_util::extract_str_array(line, field);
            for item in &arr {
                if item.to_lowercase().contains(&lq) {
                    let snippet: String = item.chars().take(80).collect();
                    results.push(format!("{} [{}]: {}", date, field, snippet));
                    if results.len() >= limit { break 'outer; }
                    break; // one match per field per session
                }
            }
        }
        // Also check scalar string field test_summary.
        let ts = crate::json_util::extract_str(line, "test_summary").unwrap_or_default();
        if ts.to_lowercase().contains(&lq) {
            let snippet: String = ts.chars().take(80).collect();
            results.push(format!("{} [test_summary]: {}", date, snippet));
            if results.len() >= limit { continue; }
        }
    }
    let truncated = scanned >= 200 && results.len() >= limit;
    if results.is_empty() {
        return format!("(no sessions found matching {:?})", query);
    }
    let mut out = format!("search={:?} results={} scanned={}", query, results.len(), scanned);
    if truncated { out.push_str(" [squeez: search truncated at 200 sessions]"); }
    out.push('\n');
    for r in &results {
        out.push_str(&format!("  {}\n", r));
    }
    out
}

/// Phase 3: show prior sessions where a file path was touched or committed.
fn tool_file_history(path: &str, limit: usize) -> String {
    if path.is_empty() {
        return "(path is empty — provide a file path substring to search)".to_string();
    }
    let sumpath = session::memory_dir().join("summaries.jsonl");
    if std::fs::metadata(&sumpath).map(|m| m.len()).unwrap_or(0) > crate::memory::MAX_FILE_BYTES {
        return "(summaries.jsonl too large — run squeez prune)".to_string();
    }
    let content = match std::fs::read_to_string(&sumpath) {
        Ok(c) => c,
        Err(_) => return "(no session history on disk)".to_string(),
    };
    let lp = path.to_lowercase();
    let mut results: Vec<String> = Vec::new();
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    for line in lines.iter().rev().take(200) {
        let date = crate::json_util::extract_str(line, "date").unwrap_or_default();
        let tokens_saved = crate::json_util::extract_u64(line, "tokens_saved").unwrap_or(0);
        let touched = crate::json_util::extract_str_array(line, "files_touched");
        let committed = crate::json_util::extract_str_array(line, "files_committed");
        let matched_touch = touched.iter().any(|f| f.to_lowercase().contains(&lp));
        let matched_commit = committed.iter().any(|f| f.to_lowercase().contains(&lp));
        if matched_touch || matched_commit {
            let committed_flag = if matched_commit { "  committed=yes" } else { "" };
            results.push(format!("{}  tokens_saved={}{}", date, tokens_saved, committed_flag));
            if results.len() >= limit { break; }
        }
    }
    if results.is_empty() {
        return format!("(no sessions found where {:?} was touched)", path);
    }
    let mut out = format!("file_history path={:?} results={}\n", path, results.len());
    for r in &results {
        out.push_str(&format!("  {}\n", r));
    }
    out
}

/// Phase 3: structured view of a specific session by date (YYYY-MM-DD).
/// Reads all matching session JSONL files (there can be multiple per day).
/// Truncates to 2 KB if the output is large.
fn tool_session_detail(date: &str) -> String {
    if date.is_empty() {
        return "(date is empty — provide YYYY-MM-DD)".to_string();
    }
    let sessions_dir = session::sessions_dir();
    let entries = match std::fs::read_dir(&sessions_dir) {
        Ok(e) => e,
        Err(_) => return "(sessions directory not found)".to_string(),
    };
    let mut matched: Vec<std::path::PathBuf> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(date) && name.ends_with(".jsonl") {
                Some(e.path())
            } else {
                None
            }
        })
        .collect();
    matched.sort();
    if matched.is_empty() {
        return format!("(no session files found for date {})", date);
    }
    let mut total_events: u64 = 0;
    let mut seen_files: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut git_events: Vec<String> = Vec::new();
    let mut test_lines: Vec<String> = Vec::new();
    for fpath in &matched {
        let fsize = std::fs::metadata(fpath).map(|m| m.len()).unwrap_or(0);
        if fsize > crate::memory::MAX_SESSION_BYTES {
            continue;
        }
        let text = match std::fs::read_to_string(fpath) {
            Ok(t) => t,
            Err(_) => continue,
        };
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            total_events += 1;
            if let Some(p) = crate::json_util::extract_str(line, "path") {
                if !seen_files.contains(&p) && seen_files.len() < 64 {
                    seen_files.push(p);
                }
            }
            let lower = line.to_lowercase();
            if (lower.contains("error[") || lower.contains("error:")) && errors.len() < 5 {
                let snip: String = line.chars().take(100).collect();
                if !errors.iter().any(|e| e == &snip) { errors.push(snip); }
            }
            if lower.contains("git") && (lower.contains("commit") || lower.contains("push")) && git_events.len() < 5 {
                git_events.push(line.chars().take(80).collect());
            }
            if (lower.contains("test result:") || (lower.contains("running") && lower.contains("test"))) && test_lines.len() < 3 {
                test_lines.push(line.chars().take(80).collect());
            }
        }
    }
    let mut out = format!(
        "session_detail date={} session_files={}\ntotal_events={}  files_seen={}\n",
        date, matched.len(), total_events, seen_files.len()
    );
    if !errors.is_empty() {
        out.push_str("errors:\n");
        for e in &errors { out.push_str(&format!("  {}\n", e)); }
    }
    if !git_events.is_empty() {
        out.push_str("git_events:\n");
        for g in &git_events { out.push_str(&format!("  {}\n", g)); }
    }
    if !test_lines.is_empty() {
        out.push_str("test_results:\n");
        for t in &test_lines { out.push_str(&format!("  {}\n", t)); }
    }
    // Truncate to 2 KB (head + tail pattern).
    if out.len() > 2048 {
        let head: String = out.chars().take(1800).collect();
        let tail_src: String = out.chars().rev().take(200).collect::<String>().chars().rev().collect();
        format!("{}\n[squeez: truncated]\n{}", head, tail_src)
    } else {
        out
    }
}

/// Phase 6: per-session compression statistics.
/// Reads dedup hit counters added by Worker 1 to context.json; defaults to 0
/// for all counters if the fields are absent (backwards-compatible).
fn tool_session_stats() -> String {
    let path = session::sessions_dir().join("context.json");
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > crate::memory::MAX_FILE_BYTES {
        return "(context.json too large)".to_string();
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return "(no context.json — session stats unavailable)".to_string(),
    };
    let exact_dedup_hits    = crate::json_util::extract_u64(&content, "exact_dedup_hits").unwrap_or(0);
    let fuzzy_dedup_hits    = crate::json_util::extract_u64(&content, "fuzzy_dedup_hits").unwrap_or(0);
    let summarize_triggers  = crate::json_util::extract_u64(&content, "summarize_triggers").unwrap_or(0);
    let intensity_ultra     = crate::json_util::extract_u64(&content, "intensity_ultra_calls").unwrap_or(0);
    let tokens_bash         = crate::json_util::extract_u64(&content, "tokens_bash").unwrap_or(0);
    let tokens_read         = crate::json_util::extract_u64(&content, "tokens_read").unwrap_or(0);
    let tokens_other        = crate::json_util::extract_u64(&content, "tokens_other").unwrap_or(0);
    let call_counter        = crate::json_util::extract_u64(&content, "call_counter").unwrap_or(0);
    format!(
        "squeez session stats\n\
exact_dedup_hits:      {}\n\
fuzzy_dedup_hits:      {}\n\
summarize_triggers:    {}\n\
intensity_ultra_calls: {}\n\
tokens_saved_bash:     {}\n\
tokens_saved_read:     {}\n\
tokens_saved_other:    {}\n\
total_calls:           {}\n",
        exact_dedup_hits, fuzzy_dedup_hits, summarize_triggers, intensity_ultra,
        tokens_bash, tokens_read, tokens_other, call_counter,
    )
}

fn tool_agent_costs() -> String {
    let ctx = load_ctx();
    crate::economy::agent_tracker::format_agent_costs(&ctx)
}

fn tool_session_efficiency() -> String {
    let ctx = load_ctx();
    let cfg = crate::config::Config::load();
    let budget = cfg.compact_threshold_tokens * 5 / 4;
    let total_in = ctx.tokens_bash + ctx.tokens_read + ctx.tokens_other;
    let dedup_hits = ctx.exact_dedup_hits + ctx.fuzzy_dedup_hits;
    // Use real tokens_saved from CurrentSession for accurate compression ratio.
    let tokens_saved = session::CurrentSession::load(&session::sessions_dir())
        .map(|c| c.tokens_saved)
        .unwrap_or(0);
    let total_out = total_in.saturating_sub(tokens_saved);
    let score = crate::economy::efficiency::compute(
        total_in,
        total_out,
        ctx.agent_estimated_tokens,
        total_in,
        dedup_hits,
        ctx.call_counter,
        budget,
    );
    crate::economy::efficiency::format_efficiency(&score)
}

/// Context pressure advisor: pressure %, calls remaining, tokens_saved, recommendation.
fn tool_context_pressure() -> String {
    let ctx = load_ctx();
    let cfg = crate::config::Config::load();
    let budget = cfg.compact_threshold_tokens * 5 / 4;
    let total_in = ctx.tokens_bash + ctx.tokens_read + ctx.tokens_other;
    let pressure_pct = if budget > 0 { (total_in * 100 / budget).min(100) } else { 0 };

    let calls_remaining_str = crate::economy::burn_rate::calls_remaining(&ctx, &cfg)
        .map(|r| format!("~{}", r))
        .unwrap_or_else(|| "unknown (need ≥3 calls)".to_string());

    let calls_left = crate::economy::burn_rate::calls_remaining(&ctx, &cfg)
        .unwrap_or(u64::MAX);

    let tokens_saved = session::CurrentSession::load(&session::sessions_dir())
        .map(|c| c.tokens_saved)
        .unwrap_or(0);

    let recommendation = if pressure_pct >= 75 || calls_left <= cfg.state_warn_calls {
        "use_state_first — save state to .claude/session_state.md, then /clear"
    } else if pressure_pct >= 55 {
        "compact_soon — run /compact to reduce context"
    } else {
        "ok"
    };

    format!(
        "context_pressure: {}%\ncalls_remaining: {}\ntokens_saved: {}\nrecommendation: {}\n",
        pressure_pct,
        calls_remaining_str,
        tokens_saved,
        recommendation,
    )
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
    fn handle_tools_list_returns_all_tools() {
        let req = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}";
        let resp = handle_request(req).expect("must respond");
        // All fourteen tool names appear in the response.
        for name in [
            "squeez_recent_calls",
            "squeez_seen_files",
            "squeez_seen_errors",
            "squeez_session_summary",
            "squeez_prior_summaries",
            "squeez_protocol",
            "squeez_seen_error_details",
            "squeez_search_history",
            "squeez_file_history",
            "squeez_session_detail",
            "squeez_session_stats",
            "squeez_agent_costs",
            "squeez_session_efficiency",
            "squeez_context_pressure",
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
        assert!(resp.contains("squeez protocol"));
        assert!(resp.contains("markers:"));
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
