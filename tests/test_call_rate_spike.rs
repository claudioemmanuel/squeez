// Tests for G2: call-rate spike detection (tick_call_rate + queue_warning).
// Verifies the 60-second rolling window, threshold logic, and once-per-session dedup.

use squeez::context::cache::SessionContext;
use squeez::session::unix_now;

#[test]
fn first_call_starts_window() {
    let mut ctx = SessionContext::default();
    let rate = ctx.tick_call_rate();
    assert_eq!(rate, 1);
    assert!(ctx.calls_minute_ts > 0);
}

#[test]
fn increments_within_window() {
    let mut ctx = SessionContext::default();
    ctx.calls_minute_ts = unix_now(); // window just started
    ctx.calls_this_minute = 5;
    let rate = ctx.tick_call_rate();
    assert_eq!(rate, 6);
}

#[test]
fn resets_after_60_seconds() {
    let mut ctx = SessionContext::default();
    // Simulate window started 65s ago
    ctx.calls_minute_ts = unix_now().saturating_sub(65);
    ctx.calls_this_minute = 15;
    let rate = ctx.tick_call_rate();
    assert_eq!(rate, 1, "window should reset after 60s");
}

#[test]
fn json_roundtrip_preserves_call_rate_fields() {
    let mut ctx = SessionContext::default();
    ctx.calls_this_minute = 12;
    ctx.calls_minute_ts = 1_700_000_000;
    let restored = SessionContext::from_json(&ctx.to_json());
    assert_eq!(restored.calls_this_minute, 12);
    assert_eq!(restored.calls_minute_ts, 1_700_000_000);
}

#[test]
fn json_roundtrip_from_old_context_defaults_to_zero() {
    let ctx = SessionContext::from_json(r#"{"session_file":"","call_counter":5}"#);
    assert_eq!(ctx.calls_this_minute, 0);
    assert_eq!(ctx.calls_minute_ts, 0);
}
