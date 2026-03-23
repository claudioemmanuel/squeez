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
