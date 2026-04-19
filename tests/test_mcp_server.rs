// Integration tests for the MCP server JSON-RPC layer. Most coverage lives
// in src/commands/mcp_server.rs#tests; this file pins the public surface of
// `handle_request` and verifies that the response wire format is something
// an MCP client could plausibly parse (no need for an actual MCP runtime).

use squeez::commands::mcp_server::handle_request;

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
