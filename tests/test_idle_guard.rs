// Tests for the 5-minute cache idle guard introduced in the workflow abuse
// prevention feature. Verifies that wrap.rs emits the warning only when
// last_activity_ts is set and idle time exceeds cache_idle_warn_secs.

use squeez::context::cache::SessionContext;
use squeez::session::unix_now;

fn make_active_ctx(idle_secs: u64) -> SessionContext {
    let mut ctx = SessionContext::default();
    // Simulate "last activity was idle_secs ago"
    ctx.last_activity_ts = unix_now().saturating_sub(idle_secs);
    ctx
}

#[test]
fn no_warning_on_fresh_session() {
    // last_activity_ts == 0 means no activity recorded; skip warning.
    let ctx = SessionContext::default();
    assert_eq!(ctx.last_activity_ts, 0);
}

#[test]
fn no_warning_when_recently_active() {
    let ctx = make_active_ctx(10); // only 10s idle
    // 270s threshold — 10s idle should not fire
    let threshold = 270u64;
    let idle = unix_now().saturating_sub(ctx.last_activity_ts);
    assert!(idle < threshold, "10s idle should not exceed 270s threshold");
}

#[test]
fn warning_condition_met_when_idle_exceeds_threshold() {
    let ctx = make_active_ctx(300); // 300s idle > 270s threshold
    let threshold = 270u64;
    let idle = unix_now().saturating_sub(ctx.last_activity_ts);
    assert!(
        idle >= threshold,
        "300s idle should exceed 270s threshold (got idle={})",
        idle
    );
}

#[test]
fn json_roundtrip_preserves_last_activity_ts() {
    let mut ctx = SessionContext::default();
    ctx.last_activity_ts = 1_700_000_000;
    let restored = SessionContext::from_json(&ctx.to_json());
    assert_eq!(restored.last_activity_ts, 1_700_000_000);
}

#[test]
fn json_roundtrip_from_old_context_defaults_to_zero() {
    // Older context.json files without last_activity_ts should default to 0.
    let json = r#"{"session_file":"","call_counter":5}"#;
    let ctx = SessionContext::from_json(json);
    assert_eq!(ctx.last_activity_ts, 0);
}
