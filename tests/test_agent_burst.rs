// Tests for parallel-agent burst detection (burst_warning in agent_tracker).
// Verifies threshold logic, window filtering, and message format.

use squeez::config::Config;
use squeez::context::cache::{AgentSpawnEntry, SessionContext};
use squeez::economy::agent_tracker::burst_warning;
use squeez::session::unix_now;

fn cfg_with_threshold(threshold: usize, window: u64) -> Config {
    let mut cfg = Config::default();
    cfg.parallel_agent_burst_threshold = threshold;
    cfg.parallel_agent_burst_window_secs = window;
    cfg
}

fn spawn_entry(ts_offset_secs: u64) -> AgentSpawnEntry {
    AgentSpawnEntry {
        call_n: 1,
        tool_name: "Agent".to_string(),
        estimated_tokens: 350_000,
        ts: unix_now().saturating_sub(ts_offset_secs),
    }
}

#[test]
fn no_warning_when_no_agents() {
    let ctx = SessionContext::default();
    let cfg = cfg_with_threshold(5, 120);
    assert!(burst_warning(&ctx, &cfg).is_none());
}

#[test]
fn no_warning_below_threshold() {
    let mut ctx = SessionContext::default();
    // 4 agents within 120s, threshold = 5
    for i in 0..4 {
        ctx.agent_spawn_log.push(spawn_entry(i * 10));
    }
    let cfg = cfg_with_threshold(5, 120);
    assert!(burst_warning(&ctx, &cfg).is_none());
}

#[test]
fn warning_at_threshold() {
    let mut ctx = SessionContext::default();
    // Exactly 5 agents within 120s, threshold = 5
    for i in 0..5 {
        ctx.agent_spawn_log.push(spawn_entry(i * 10));
    }
    let cfg = cfg_with_threshold(5, 120);
    let w = burst_warning(&ctx, &cfg);
    assert!(w.is_some(), "should warn at threshold");
    let msg = w.unwrap();
    assert!(msg.contains("WORKFLOW BURST"));
    assert!(msg.contains("5 agents"));
}

#[test]
fn agents_outside_window_dont_count() {
    let mut ctx = SessionContext::default();
    // 3 agents within window, 3 agents outside window
    for i in 0..3 {
        ctx.agent_spawn_log.push(spawn_entry(i * 10)); // within 120s
    }
    for i in 0..3 {
        ctx.agent_spawn_log.push(spawn_entry(200 + i * 10)); // outside 120s
    }
    let cfg = cfg_with_threshold(5, 120);
    assert!(
        burst_warning(&ctx, &cfg).is_none(),
        "agents outside window should not count toward burst"
    );
}

#[test]
fn warning_message_contains_token_estimate() {
    let mut ctx = SessionContext::default();
    for i in 0..6 {
        ctx.agent_spawn_log.push(spawn_entry(i * 5));
    }
    let cfg = cfg_with_threshold(5, 120);
    let msg = burst_warning(&ctx, &cfg).expect("should warn");
    // 6 agents × 350_000 / 1000 = 2100K
    assert!(msg.contains("2100K"), "expected ~2100K in message, got: {}", msg);
}

#[test]
fn threshold_zero_disables_warning() {
    let mut ctx = SessionContext::default();
    for i in 0..10 {
        ctx.agent_spawn_log.push(spawn_entry(i));
    }
    let cfg = cfg_with_threshold(0, 120);
    assert!(burst_warning(&ctx, &cfg).is_none(), "threshold=0 should disable the warning");
}
