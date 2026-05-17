// Integration tests for the auto-curation nudge engine.
//
// Unit tests for the in-memory state machine live alongside the module in
// src/economy/nudge.rs. This file covers the end-to-end persistence path:
// counters survive a save → reload cycle on context.json, and the same
// nudge does not re-fire after a reload.

use squeez::config::Config;
use squeez::context::cache::{FileAccess, SessionContext};
use squeez::economy::nudge;

#[test]
fn nudge_counters_round_trip_via_json() {
    let mut ctx = SessionContext::default();
    let cfg = Config::default();
    let err = vec!["error: boom".to_string()];

    // Bump error counter twice (below threshold). Then write/read context.json.
    nudge::evaluate(&mut ctx, "cmd", &[], FileAccess::Read, &err, &cfg);
    nudge::evaluate(&mut ctx, "cmd", &[], FileAccess::Read, &err, &cfg);

    let json = ctx.to_json();
    let restored = SessionContext::from_json(&json);

    assert_eq!(restored.error_count_fp.len(), 1);
    assert_eq!(restored.error_count_n[0], 2);
}

#[test]
fn nudge_fires_after_reload_when_threshold_crossed() {
    // Simulate the per-call sub-process boundary: bump twice, persist, reload,
    // bump once more — the nudge must fire on this third call.
    let cfg = Config::default();
    let err = vec!["error: oops".to_string()];

    let mut ctx = SessionContext::default();
    nudge::evaluate(&mut ctx, "cmd", &[], FileAccess::Read, &err, &cfg);
    nudge::evaluate(&mut ctx, "cmd", &[], FileAccess::Read, &err, &cfg);

    let json = ctx.to_json();
    let mut restored = SessionContext::from_json(&json);

    let hints = nudge::evaluate(&mut restored, "cmd", &[], FileAccess::Read, &err, &cfg);
    assert_eq!(hints.len(), 1);
    assert!(hints[0].contains("seen ×3"));
}

#[test]
fn nudge_does_not_repeat_after_reload() {
    // Once a nudge has been emitted, it must stay quiet on subsequent calls
    // — even after the context is round-tripped through disk.
    let cfg = Config::default();
    let err = vec!["error: x".to_string()];

    let mut ctx = SessionContext::default();
    for _ in 0..3 {
        nudge::evaluate(&mut ctx, "cmd", &[], FileAccess::Read, &err, &cfg);
    }
    assert!(!ctx.nudged_keys.is_empty(), "first burst should mark the key");

    let json = ctx.to_json();
    let mut restored = SessionContext::from_json(&json);

    // Two more bumps after reload — no fresh hint expected.
    let h1 = nudge::evaluate(&mut restored, "cmd", &[], FileAccess::Read, &err, &cfg);
    let h2 = nudge::evaluate(&mut restored, "cmd", &[], FileAccess::Read, &err, &cfg);
    assert!(h1.is_empty());
    assert!(h2.is_empty());
}

#[test]
fn legacy_context_without_nudge_fields_still_loads() {
    // Pre-existing context.json files written by older squeez versions don't
    // have the nudge fields. Loading them must not blow up — and the nudge
    // counters must initialize to empty.
    let legacy = r#"{"session_file":"old.jsonl","call_counter":3,
"call_log_n":[],"call_log_cmd":[],"call_log_hash":[],"call_log_len":[],"call_log_short":[],
"call_log_shingles":[],
"seen_files_path":[],"seen_files_size":[],"seen_files_last":[],"seen_files_access":[],
"seen_errors":[],"error_snippet_fp":[],"error_snippet_text":[],
"seen_git_refs":[],
"tokens_bash":0,"tokens_read":0,"tokens_grep":0,"tokens_other":0,"reread_count":0,
"exact_dedup_hits":0,"fuzzy_dedup_hits":0,"summarize_triggers":0,"intensity_ultra_calls":0,
"agent_spawns":0,"agent_estimated_tokens":0,
"agent_spawn_log_call_n":[],"agent_spawn_log_tool":[],"agent_spawn_log_tokens":[],"agent_spawn_log_ts":[],
"burn_window_call_n":[],"burn_window_tokens":[],"burn_window_ts":[]}"#;

    let c = SessionContext::from_json(legacy);
    assert_eq!(c.call_counter, 3);
    assert!(c.error_count_fp.is_empty());
    assert!(c.error_count_n.is_empty());
    assert!(c.file_mod_path.is_empty());
    assert!(c.cmd_repeat_name.is_empty());
    assert!(c.nudged_keys.is_empty());
}

#[test]
fn file_mod_nudge_triggers_only_on_writes() {
    let cfg = Config::default();
    let files = vec!["/src/foo.rs".to_string()];

    let mut ctx = SessionContext::default();
    // Five reads → no nudge, no counter increment.
    for _ in 0..5 {
        nudge::evaluate(&mut ctx, "less /src/foo.rs", &files, FileAccess::Read, &[], &cfg);
    }
    assert!(ctx.file_mod_path.is_empty());

    // Five writes → exactly one nudge on the fifth.
    let mut any = false;
    for i in 1..=5 {
        let h = nudge::evaluate(&mut ctx, "sed -i ...", &files, FileAccess::Write, &[], &cfg);
        if i == 5 {
            assert_eq!(h.len(), 1, "should nudge exactly on 5th write");
            assert!(h[0].contains("/src/foo.rs"));
            any = true;
        }
    }
    assert!(any);
}

#[test]
fn cmd_repeat_nudge_triggers_for_expensive_cmds_only() {
    let cfg = Config::default();
    let mut ctx = SessionContext::default();

    // `ls` is cheap → never nudges.
    for _ in 0..20 {
        let h = nudge::evaluate(&mut ctx, "ls -la", &[], FileAccess::Read, &[], &cfg);
        assert!(h.is_empty());
    }
    assert!(ctx.cmd_repeat_name.is_empty());

    // `cargo` is expensive. Default threshold = 4.
    for i in 1..=4 {
        let h = nudge::evaluate(&mut ctx, "cargo test", &[], FileAccess::Read, &[], &cfg);
        if i == 4 {
            assert_eq!(h.len(), 1);
            assert!(h[0].contains("cargo"));
        } else {
            assert!(h.is_empty());
        }
    }
}

#[test]
fn nudge_thresholds_respect_config_overrides() {
    let mut cfg = Config::default();
    cfg.nudge_error_threshold = 2;
    let err = vec!["error: zap".to_string()];

    let mut ctx = SessionContext::default();
    let h1 = nudge::evaluate(&mut ctx, "cmd", &[], FileAccess::Read, &err, &cfg);
    let h2 = nudge::evaluate(&mut ctx, "cmd", &[], FileAccess::Read, &err, &cfg);
    assert!(h1.is_empty());
    assert_eq!(h2.len(), 1, "with threshold=2 the second occurrence fires");
}
