/// Tests for phase 6: compression statistics tracking in SessionContext.
use squeez::context::cache::SessionContext;

#[test]
fn exact_dedup_hit_increments_counter() {
    let mut ctx = SessionContext::default();
    assert_eq!(ctx.exact_dedup_hits, 0);
    ctx.note_redundancy_hit_exact();
    assert_eq!(ctx.exact_dedup_hits, 1);
    ctx.note_redundancy_hit_exact();
    assert_eq!(ctx.exact_dedup_hits, 2);
}

#[test]
fn fuzzy_dedup_hit_increments_counter() {
    let mut ctx = SessionContext::default();
    assert_eq!(ctx.fuzzy_dedup_hits, 0);
    ctx.note_redundancy_hit_fuzzy();
    assert_eq!(ctx.fuzzy_dedup_hits, 1);
}

#[test]
fn summarize_trigger_increments_counter() {
    let mut ctx = SessionContext::default();
    assert_eq!(ctx.summarize_triggers, 0);
    ctx.note_summarize_trigger();
    ctx.note_summarize_trigger();
    assert_eq!(ctx.summarize_triggers, 2);
}

#[test]
fn intensity_ultra_increments_counter() {
    let mut ctx = SessionContext::default();
    assert_eq!(ctx.intensity_ultra_calls, 0);
    ctx.note_intensity_ultra();
    assert_eq!(ctx.intensity_ultra_calls, 1);
}

#[test]
fn stat_counters_survive_json_round_trip() {
    let mut ctx = SessionContext::default();
    ctx.note_redundancy_hit_exact();
    ctx.note_redundancy_hit_exact();
    ctx.note_redundancy_hit_fuzzy();
    ctx.note_summarize_trigger();
    ctx.note_intensity_ultra();
    ctx.note_intensity_ultra();

    let json = ctx.to_json();
    let loaded = SessionContext::from_json(&json);
    assert_eq!(loaded.exact_dedup_hits, 2);
    assert_eq!(loaded.fuzzy_dedup_hits, 1);
    assert_eq!(loaded.summarize_triggers, 1);
    assert_eq!(loaded.intensity_ultra_calls, 2);
}

#[test]
fn legacy_json_without_stat_fields_loads_with_zero() {
    let json = r#"{"session_file":"test.jsonl","call_counter":0,
"call_log_n":[],"call_log_cmd":[],"call_log_hash":[],"call_log_len":[],
"call_log_short":[],"call_log_shingles":[],
"seen_files_path":[],"seen_files_size":[],"seen_files_last":[],
"seen_errors":[],"seen_git_refs":[],
"tokens_bash":100,"tokens_read":200,"tokens_other":50}"#;
    let ctx = SessionContext::from_json(json);
    assert_eq!(ctx.exact_dedup_hits, 0);
    assert_eq!(ctx.fuzzy_dedup_hits, 0);
    assert_eq!(ctx.summarize_triggers, 0);
    assert_eq!(ctx.intensity_ultra_calls, 0);
    // Existing fields still load correctly.
    assert_eq!(ctx.tokens_bash, 100);
    assert_eq!(ctx.tokens_read, 200);
}

#[test]
fn stat_counters_saturate_not_overflow() {
    let mut ctx = SessionContext::default();
    ctx.exact_dedup_hits = u32::MAX;
    ctx.note_redundancy_hit_exact();
    assert_eq!(ctx.exact_dedup_hits, u32::MAX, "saturating_add: no overflow");
}
