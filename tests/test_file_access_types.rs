/// Tests for phase 4: FileAccess enum and typed file tracking.
use squeez::context::cache::{FileAccess, SessionContext};

#[test]
fn note_file_read_sets_access() {
    let mut ctx = SessionContext::default();
    ctx.next_call_n();
    ctx.note_file("src/main.rs", FileAccess::Read);
    assert_eq!(ctx.seen_files[0].access, FileAccess::Read);
}

#[test]
fn note_file_write_sets_access() {
    let mut ctx = SessionContext::default();
    ctx.next_call_n();
    ctx.note_file("src/lib.rs", FileAccess::Write);
    assert_eq!(ctx.seen_files[0].access, FileAccess::Write);
}

#[test]
fn note_file_created_sets_access() {
    let mut ctx = SessionContext::default();
    ctx.next_call_n();
    ctx.note_file("src/new.rs", FileAccess::Created);
    assert_eq!(ctx.seen_files[0].access, FileAccess::Created);
}

#[test]
fn note_file_deleted_sets_access() {
    let mut ctx = SessionContext::default();
    ctx.next_call_n();
    ctx.note_file("src/old.rs", FileAccess::Deleted);
    assert_eq!(ctx.seen_files[0].access, FileAccess::Deleted);
}

#[test]
fn note_files_batch_defaults_to_read() {
    let mut ctx = SessionContext::default();
    ctx.next_call_n();
    ctx.note_files(&["a.rs".to_string(), "b.rs".to_string()]);
    for f in &ctx.seen_files {
        assert_eq!(f.access, FileAccess::Read, "batch note_files should default to Read");
    }
}

#[test]
fn update_existing_file_changes_access_type() {
    let mut ctx = SessionContext::default();
    ctx.next_call_n();
    ctx.note_file("src/main.rs", FileAccess::Read);
    ctx.next_call_n();
    ctx.note_file("src/main.rs", FileAccess::Write);
    assert_eq!(ctx.seen_files.len(), 1, "deduped");
    assert_eq!(ctx.seen_files[0].access, FileAccess::Write, "access updated to latest");
}

#[test]
fn file_access_as_char_round_trips() {
    for access in [FileAccess::Read, FileAccess::Write, FileAccess::Created, FileAccess::Deleted] {
        let c = access.as_char();
        assert_eq!(FileAccess::from_char(c), access);
    }
}

#[test]
fn access_survives_json_round_trip() {
    let mut ctx = SessionContext::default();
    ctx.next_call_n();
    ctx.note_file("src/lib.rs", FileAccess::Write);
    let json = ctx.to_json();
    let loaded = SessionContext::from_json(&json);
    assert_eq!(loaded.seen_files[0].access, FileAccess::Write);
}

#[test]
fn legacy_json_without_access_field_defaults_to_read() {
    let json = r#"{"session_file":"test.jsonl","call_counter":1,
"call_log_n":[],"call_log_cmd":[],"call_log_hash":[],"call_log_len":[],
"call_log_short":[],"call_log_shingles":[],
"seen_files_path":["src/main.rs"],"seen_files_size":[0],"seen_files_last":[1],
"seen_errors":[],"seen_git_refs":[],
"tokens_bash":0,"tokens_read":0,"tokens_other":0,
"exact_dedup_hits":0,"fuzzy_dedup_hits":0,"summarize_triggers":0,"intensity_ultra_calls":0}"#;
    let ctx = SessionContext::from_json(json);
    assert_eq!(ctx.seen_files.len(), 1);
    assert_eq!(ctx.seen_files[0].access, FileAccess::Read, "default to Read for legacy entries");
}
