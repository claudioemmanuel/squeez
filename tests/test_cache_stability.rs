//! Cache-stability audit (P3, issue #7).
//!
//! Claude's prompt caching invalidates tools → system → messages. Anything
//! squeez emits that lands in a cacheable layer must be byte-stable across
//! calls within a session so the cache stays warm. A regression here can
//! silently 10× the cost of a session.
//!
//! These tests lock in the invariant. Future edits that break byte-stability
//! must either (a) prove the changed surface is **not** cacheable, or
//! (b) come with a release note explaining the cache-cost impact.
//!
//! Surfaces audited:
//! - `protocol::full_payload()` — self-teach payload returned by the
//!   `squeez_protocol` MCP tool.
//! - MCP `tools/list` response — tool definitions live in the cacheable
//!   tools layer; the only allowed per-call variation is the JSON-RPC `id`.
//! - MCP `initialize` response — sent once per MCP session; only `id`
//!   should vary.
//! - `persona::text` — `include_str!` constants; should be identity-stable.

use squeez::commands::{mcp_server, persona, protocol};

/// Strip the JSON-RPC `id` field from a response so we can compare just the
/// payload across calls. Naive substring strip — sufficient for these tests
/// because all responses have a single top-level `"id":<value>,` field.
fn strip_id(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '"' && s[i..].starts_with("\"id\":") {
            // Skip the `"id":<value>,` slice up to the next `,` or `}`.
            // Move chars iterator past `"id":`.
            for _ in 0..4 {
                chars.next();
            }
            // Now scan until we see `,` (still inside the object) or `}`.
            let mut depth = 0i32;
            for (_, c2) in chars.by_ref() {
                match c2 {
                    '{' | '[' => depth += 1,
                    '}' | ']' => {
                        if depth == 0 {
                            out.push(c2);
                            break;
                        }
                        depth -= 1;
                    }
                    ',' if depth == 0 => break,
                    _ => {}
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[test]
fn protocol_payload_is_byte_stable_across_calls() {
    let first = protocol::full_payload();
    for _ in 0..100 {
        assert_eq!(
            protocol::full_payload(),
            first,
            "protocol::full_payload must return byte-identical content across calls",
        );
    }
}

#[test]
fn protocol_payload_constants_are_compile_time_stable() {
    // SQUEEZ_PROTOCOL and SQUEEZ_MARKERS_SPEC are pub const &'static str
    // and used directly inside MCP responses. Reading them twice must yield
    // the same slice content.
    assert_eq!(protocol::SQUEEZ_PROTOCOL, protocol::SQUEEZ_PROTOCOL);
    assert_eq!(protocol::SQUEEZ_MARKERS_SPEC, protocol::SQUEEZ_MARKERS_SPEC);
    // And they must compose into the full payload byte-for-byte.
    let composed = format!(
        "{}\n{}",
        protocol::SQUEEZ_PROTOCOL,
        protocol::SQUEEZ_MARKERS_SPEC
    );
    assert_eq!(protocol::full_payload(), composed);
}

#[test]
fn persona_text_is_identity_stable() {
    for p in [
        persona::Persona::Off,
        persona::Persona::Lite,
        persona::Persona::Full,
        persona::Persona::Ultra,
    ] {
        let a = persona::text(p);
        let b = persona::text(p);
        // `include_str!` returns &'static str — same backing slice every call.
        assert_eq!(a.as_ptr(), b.as_ptr(), "persona::text({:?}) must be a stable static slice", p);
        assert_eq!(a, b);
    }
}

#[test]
fn mcp_tools_list_payload_is_stable_modulo_id() {
    let req_a = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}";
    let req_b = "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"tools/list\"}";
    let req_c = "{\"jsonrpc\":\"2.0\",\"id\":\"abc-123\",\"method\":\"tools/list\"}";

    let resp_a = mcp_server::handle_request(req_a).expect("response");
    let resp_b = mcp_server::handle_request(req_b).expect("response");
    let resp_c = mcp_server::handle_request(req_c).expect("response");

    let stripped_a = strip_id(&resp_a);
    let stripped_b = strip_id(&resp_b);
    let stripped_c = strip_id(&resp_c);

    assert_eq!(
        stripped_a, stripped_b,
        "tools/list payload must be byte-stable across numeric ids — anything else invalidates the MCP tools cache layer",
    );
    assert_eq!(
        stripped_a, stripped_c,
        "tools/list payload must be byte-stable across string ids",
    );

    // Sanity: response actually contains a tools array.
    assert!(stripped_a.contains("\"tools\":["), "missing tools array: {}", stripped_a);
    assert!(stripped_a.contains("squeez_recent_calls"), "tools array empty");
}

#[test]
fn mcp_initialize_payload_is_stable_modulo_id() {
    let req_a = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}";
    let req_b = "{\"jsonrpc\":\"2.0\",\"id\":99,\"method\":\"initialize\"}";
    let resp_a = mcp_server::handle_request(req_a).expect("response");
    let resp_b = mcp_server::handle_request(req_b).expect("response");
    assert_eq!(strip_id(&resp_a), strip_id(&resp_b));
    // Sanity.
    assert!(strip_id(&resp_a).contains("\"protocolVersion\":\"2024-11-05\""));
}

#[test]
fn mcp_protocol_tool_payload_is_stable_across_calls() {
    let req = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\
\"params\":{\"name\":\"squeez_protocol\",\"arguments\":{}}}";
    let first = mcp_server::handle_request(req).expect("response");
    for _ in 0..20 {
        let next = mcp_server::handle_request(req).expect("response");
        assert_eq!(next, first, "squeez_protocol tool result must be byte-stable");
    }
}

#[test]
fn strip_id_helper_works() {
    assert_eq!(
        strip_id("{\"jsonrpc\":\"2.0\",\"id\":42,\"result\":{}}"),
        "{\"jsonrpc\":\"2.0\",\"result\":{}}",
    );
    assert_eq!(
        strip_id("{\"jsonrpc\":\"2.0\",\"id\":\"abc\",\"result\":{}}"),
        "{\"jsonrpc\":\"2.0\",\"result\":{}}",
    );
}
