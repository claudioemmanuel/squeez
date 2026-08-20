// Integration tests for the MCP server JSON-RPC layer. Most coverage lives
// in src/commands/mcp_server.rs#tests; this file pins the public surface of
// `handle_request` and verifies that the response wire format is something
// an MCP client could plausibly parse (no need for an actual MCP runtime).

use std::sync::Mutex;

use squeez::commands::mcp_server::handle_request;

// SQUEEZ_DIR is process-global — serialise every test that mutates it so
// parallel `cargo test` threads don't race.
static ENV_GUARD: Mutex<()> = Mutex::new(());

fn assert_jsonrpc_response(resp: &str, expected_id: &str) {
    assert!(resp.starts_with('{'), "should be a JSON object");
    assert!(resp.ends_with('}'), "should be a JSON object");
    assert!(resp.contains("\"jsonrpc\":\"2.0\""));
    assert!(resp.contains(&format!("\"id\":{}", expected_id)));
}

#[test]
fn initialize_returns_server_info() {
    let req = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}";
    let resp = handle_request(req).expect("must respond");
    assert_jsonrpc_response(&resp, "1");
    assert!(resp.contains("\"protocolVersion\""));
    assert!(resp.contains("\"name\":\"squeez\""));
    assert!(resp.contains("\"capabilities\""));
}

#[test]
fn tools_list_advertises_six_tools() {
    let req = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}";
    let resp = handle_request(req).expect("must respond");
    assert_jsonrpc_response(&resp, "2");
    for tool in [
        "squeez_recent_calls",
        "squeez_seen_files",
        "squeez_seen_errors",
        "squeez_session_summary",
        "squeez_prior_summaries",
        "squeez_protocol",
    ] {
        assert!(resp.contains(tool), "tools/list missing {}", tool);
    }
}

#[test]
fn tools_call_protocol_returns_payload_text() {
    let req = "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\
\"params\":{\"name\":\"squeez_protocol\",\"arguments\":{}}}";
    let resp = handle_request(req).expect("must respond");
    assert_jsonrpc_response(&resp, "3");
    assert!(resp.contains("\"content\""));
    assert!(resp.contains("\"type\":\"text\""));
    assert!(resp.contains("squeez protocol"));
}

#[test]
fn tools_call_recent_calls_returns_text_block() {
    let req = "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\
\"params\":{\"name\":\"squeez_recent_calls\",\"arguments\":{\"n\":3}}}";
    let resp = handle_request(req).expect("must respond");
    assert_jsonrpc_response(&resp, "4");
    assert!(resp.contains("\"content\""));
    // Either we have call data ("session=") or the empty-state message.
    assert!(
        resp.contains("session=") || resp.contains("no calls recorded"),
        "unexpected payload: {}",
        resp
    );
}

#[test]
fn unknown_method_returns_error_minus_32601() {
    let req = "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"this/does/not/exist\"}";
    let resp = handle_request(req).expect("must respond");
    assert_jsonrpc_response(&resp, "5");
    assert!(resp.contains("\"error\""));
    assert!(resp.contains("-32601"));
}

#[test]
fn tools_call_unknown_tool_returns_error() {
    let req = "{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"tools/call\",\
\"params\":{\"name\":\"not_a_tool\",\"arguments\":{}}}";
    let resp = handle_request(req).expect("must respond");
    assert_jsonrpc_response(&resp, "6");
    assert!(resp.contains("\"error\""));
    assert!(resp.contains("unknown tool"));
}

#[test]
fn notifications_get_no_response() {
    // Per JSON-RPC 2.0, requests without `id` are notifications and must NOT
    // be answered.
    let req = "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}";
    assert!(handle_request(req).is_none());
}

#[test]
fn ping_returns_empty_result() {
    let req = "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}";
    let resp = handle_request(req).expect("must respond");
    assert_jsonrpc_response(&resp, "7");
    assert!(resp.contains("\"result\":{}"));
}

#[test]
fn string_id_is_echoed_back_quoted() {
    let req = "{\"jsonrpc\":\"2.0\",\"id\":\"req-1\",\"method\":\"initialize\"}";
    let resp = handle_request(req).expect("must respond");
    assert!(resp.contains("\"id\":\"req-1\""));
}

#[test]
fn session_summary_tool_works() {
    let req = "{\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"tools/call\",\
\"params\":{\"name\":\"squeez_session_summary\",\"arguments\":{}}}";
    let resp = handle_request(req).expect("must respond");
    assert_jsonrpc_response(&resp, "8");
    assert!(resp.contains("\"content\""));
    // Returns at minimum the session_file / call_counter / tokens_bash labels.
    assert!(resp.contains("session_file") || resp.contains("call_counter"));
}

/// E1: `squeez_session_efficiency` must surface the honest overhead ledger —
/// cumulative squeez-authored token cost and the signed net_saved (tokens
/// saved minus overhead), which can go negative when overhead outweighs
/// compression.
#[test]
fn session_efficiency_exposes_overhead_tokens() {
    let _g = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());

    let dir = std::env::temp_dir().join(format!(
        "squeez_mcp_efficiency_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let sessions = dir.join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();

    let s = squeez::session::CurrentSession {
        session_file: "test.jsonl".to_string(),
        total_tokens: 1000,
        tokens_saved: 200,
        total_calls: 10,
        compact_warned: false,
        state_warned: false,
        start_ts: 1_000,
        overhead_tokens: 777,
    };
    s.save(&sessions);

    let prev = std::env::var("SQUEEZ_DIR").ok();
    std::env::set_var("SQUEEZ_DIR", &dir);

    let req = "{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"tools/call\",\
\"params\":{\"name\":\"squeez_session_efficiency\",\"arguments\":{}}}";
    let resp = handle_request(req).expect("must respond");

    match prev {
        Some(v) => std::env::set_var("SQUEEZ_DIR", v),
        None => std::env::remove_var("SQUEEZ_DIR"),
    }
    std::fs::remove_dir_all(&dir).ok();

    assert_jsonrpc_response(&resp, "9");
    assert!(resp.contains("Overhead tokens"), "missing overhead label: {}", resp);
    assert!(resp.contains("777"), "missing overhead value: {}", resp);
    assert!(resp.contains("Net saved"), "missing net_saved label: {}", resp);
    // net_saved = tokens_saved(200) - overhead_tokens(777) = -577
    assert!(resp.contains("-577"), "missing signed net_saved: {}", resp);
}

/// Regression for #210: a stashed blob containing tab indentation used to be
/// spliced into the JSON-RPC frame with the tab still literal, and the MCP
/// client rejected the whole response ("Invalid control character at:").
/// This is the reporter's exact repro, driven through the public dispatcher.
#[test]
fn retrieve_response_with_control_chars_is_valid_json() {
    let _g = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("squeez-mcp-ctrl-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let prev = std::env::var("SQUEEZ_DIR").ok();
    std::env::set_var("SQUEEZ_DIR", &dir);

    let original = "fn main() {\n\tlet s = \"tab\\tinside\";\r\n}\u{7}";
    let key = squeez::context::retrieve::store(original).expect("blob must store");
    let req = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":42,\"method\":\"tools/call\",\
\"params\":{{\"name\":\"squeez_retrieve\",\"arguments\":{{\"key\":\"{}\"}}}}}}",
        key
    );
    let resp = handle_request(&req).expect("must respond");

    match prev {
        Some(v) => std::env::set_var("SQUEEZ_DIR", v),
        None => std::env::remove_var("SQUEEZ_DIR"),
    }
    std::fs::remove_dir_all(&dir).ok();

    assert_jsonrpc_response(&resp, "42");
    assert!(
        !resp.contains('\t'),
        "raw tab leaked into the JSON-RPC frame: {:?}",
        resp
    );
    let parsed = squeez::json_util::parse_value(&resp)
        .unwrap_or_else(|| panic!("response must be parseable JSON: {:?}", resp));
    let text = parsed
        .get("result")
        .and_then(|r| r.get("content"))
        .map(|c| c.as_arr())
        .and_then(|a| a.first())
        .map(|e| e.get_str("text"))
        .expect("result.content[0].text");
    assert_eq!(text, original, "blob must round-trip byte-for-byte");
}
