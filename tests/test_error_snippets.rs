/// Tests for phase 2: error text preservation in SessionContext.
use squeez::context::cache::SessionContext;

#[test]
fn snippet_stored_on_first_occurrence() {
    let mut ctx = SessionContext::default();
    ctx.note_errors(&["error: cannot find function 'foo'".to_string()]);
    assert_eq!(ctx.seen_errors.len(), 1);
    assert_eq!(ctx.error_snippets.len(), 1);
    assert!(ctx.error_snippets[0].1.contains("cannot find function"));
}

#[test]
fn snippet_not_duplicated_on_repeat_error() {
    let mut ctx = SessionContext::default();
    let err = "error[E0308]: mismatched types".to_string();
    ctx.note_errors(&[err.clone()]);
    ctx.note_errors(&[err.clone()]);
    assert_eq!(ctx.seen_errors.len(), 1, "fingerprint deduped");
    assert_eq!(ctx.error_snippets.len(), 1, "snippet deduped");
}

#[test]
fn snippet_capped_at_128_chars() {
    let mut ctx = SessionContext::default();
    let long_err = "error: ".to_string() + &"x".repeat(200);
    ctx.note_errors(&[long_err]);
    assert_eq!(ctx.error_snippets.len(), 1);
    assert!(ctx.error_snippets[0].1.len() <= 128);
}

#[test]
fn snippet_fp_matches_seen_errors_fp() {
    let mut ctx = SessionContext::default();
    ctx.note_errors(&["error: foo".to_string()]);
    // The fingerprint stored in error_snippets must equal the one in seen_errors.
    assert_eq!(ctx.error_snippets[0].0, ctx.seen_errors[0]);
}

#[test]
fn multiple_distinct_errors_all_stored() {
    let mut ctx = SessionContext::default();
    ctx.note_errors(&[
        "error: foo".to_string(),
        "error: bar".to_string(),
        "error: baz".to_string(),
    ]);
    assert_eq!(ctx.error_snippets.len(), 3);
    assert_eq!(ctx.seen_errors.len(), 3);
}

#[test]
fn snippets_survive_json_round_trip() {
    let mut ctx = SessionContext::default();
    // Use an error without square brackets to avoid the hand-rolled JSON parser
    // limitation (extract_str_array uses ']' as array terminator).
    // Real-world errors with [E0308] have brackets sanitized to (E0308) in snippets.
    ctx.note_errors(&["error E0308 mismatched types at src/main.rs:42".to_string()]);
    let json = ctx.to_json();
    let loaded = SessionContext::from_json(&json);
    assert_eq!(loaded.error_snippets.len(), 1);
    assert!(loaded.error_snippets[0].1.contains("mismatched types"));
    assert_eq!(loaded.error_snippets[0].0, loaded.seen_errors[0]);
}

#[test]
fn legacy_json_without_snippets_loads_clean() {
    // Old context.json without error_snippet_* fields should load with empty snippets.
    let json = r#"{"session_file":"test.jsonl","call_counter":0,
"call_log_n":[],"call_log_cmd":[],"call_log_hash":[],"call_log_len":[],
"call_log_short":[],"call_log_shingles":[],
"seen_files_path":[],"seen_files_size":[],"seen_files_last":[],
"seen_errors":[12345678],"seen_git_refs":[],
"tokens_bash":0,"tokens_read":0,"tokens_other":0,
"exact_dedup_hits":0,"fuzzy_dedup_hits":0,"summarize_triggers":0,"intensity_ultra_calls":0}"#;
    let ctx = SessionContext::from_json(json);
    assert_eq!(ctx.seen_errors.len(), 1, "fingerprint loaded");
    assert_eq!(ctx.error_snippets.len(), 0, "snippets absent — backward compat");
}
