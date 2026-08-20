#[test]
fn test_extract_str_basic() {
    let s = r#"{"name":"hello","other":"world"}"#;
    assert_eq!(
        squeez::json_util::extract_str(s, "name"),
        Some("hello".to_string())
    );
    assert_eq!(
        squeez::json_util::extract_str(s, "other"),
        Some("world".to_string())
    );
}

#[test]
fn test_extract_str_missing_key() {
    assert_eq!(
        squeez::json_util::extract_str(r#"{"name":"hello"}"#, "missing"),
        None
    );
}

#[test]
fn test_extract_u64_basic() {
    let s = r#"{"count":42,"other":7}"#;
    assert_eq!(squeez::json_util::extract_u64(s, "count"), Some(42));
    assert_eq!(squeez::json_util::extract_u64(s, "other"), Some(7));
}

#[test]
fn test_extract_u64_missing() {
    assert_eq!(squeez::json_util::extract_u64(r#"{"x":1}"#, "y"), None);
}

#[test]
fn test_extract_bool_true_false() {
    let s = r#"{"enabled":true,"flag":false}"#;
    assert_eq!(squeez::json_util::extract_bool(s, "enabled"), Some(true));
    assert_eq!(squeez::json_util::extract_bool(s, "flag"), Some(false));
}

#[test]
fn test_extract_str_array_basic() {
    let s = r#"{"files":["src/foo.rs","src/bar.rs"]}"#;
    let v = squeez::json_util::extract_str_array(s, "files");
    assert_eq!(v, vec!["src/foo.rs", "src/bar.rs"]);
}

#[test]
fn test_extract_str_array_empty() {
    let s = r#"{"files":[]}"#;
    assert!(squeez::json_util::extract_str_array(s, "files").is_empty());
}

#[test]
fn test_escape_str_quotes_and_newlines() {
    let s = "line1\nline\"2\"";
    let escaped = squeez::json_util::escape_str(s);
    assert!(escaped.contains("\\n"));
    assert!(escaped.contains("\\\""));
}

#[test]
fn test_str_array_serialization() {
    let items = vec!["a".to_string(), "b/c.rs".to_string()];
    let json = squeez::json_util::str_array(&items);
    assert_eq!(json, r#"["a","b/c.rs"]"#);
}

#[test]
fn test_str_array_empty() {
    let json = squeez::json_util::str_array(&[]);
    assert_eq!(json, "[]");
}

// ── Regression: control characters must not leak into JSON strings (#210) ──
//
// MCP responses are assembled by hand, so a literal tab inside a retrieved
// blob used to travel straight into the JSON-RPC frame and the client
// rejected the whole response with "Invalid control character at:".

#[test]
fn test_escape_str_tab() {
    assert_eq!(squeez::json_util::escape_str("a\tb"), "a\\tb");
}

#[test]
fn test_escape_str_all_named_escapes() {
    assert_eq!(
        squeez::json_util::escape_str("\u{8}\t\n\u{c}\r\"\\"),
        "\\b\\t\\n\\f\\r\\\"\\\\"
    );
}

#[test]
fn test_escape_str_carriage_return_is_preserved_not_dropped() {
    // Previously `\r` was silently deleted, which corrupted CRLF payloads.
    assert_eq!(squeez::json_util::escape_str("a\r\nb"), "a\\r\\nb");
}

#[test]
fn test_escape_str_other_control_chars_use_u_escape() {
    assert_eq!(squeez::json_util::escape_str("\0"), "\\u0000");
    assert_eq!(squeez::json_util::escape_str("\u{1}\u{1f}"), "\\u0001\\u001f");
}

#[test]
fn test_escape_str_leaves_printable_and_unicode_alone() {
    let s = "ok — café 日本語 {}[]";
    assert_eq!(squeez::json_util::escape_str(s), s);
}

#[test]
fn test_escaped_control_chars_reparse_as_json() {
    // The end-to-end property the bug violated: whatever escape_str emits must
    // survive a round-trip through the parser as the ORIGINAL bytes.
    let raw = "fn main() {\n\tlet x = \"q\";\r\n}\u{7}\0";
    let doc = format!("{{\"text\":\"{}\"}}", squeez::json_util::escape_str(raw));
    let parsed = squeez::json_util::parse_value(&doc).expect("escaped payload must parse");
    assert_eq!(parsed.get_str("text"), raw);
}
