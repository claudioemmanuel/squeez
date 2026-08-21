// Regression tests for the 2026-08-21 token-burn post-mortem.
//
// Each test below fails against the pre-fix code. They cover the four defects
// that let a 26-agent, depth-4 fan-out spend ~302M tokens while squeez's own
// counters reported almost nothing.

use squeez::config::Config;
use squeez::context::cache::SessionContext;
use squeez::economy::agent_tracker;

fn tmp_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("squeez_agentguard_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// ── D6: flat spawn cost ────────────────────────────────────────────────────

#[test]
fn spawn_cost_falls_back_to_config_when_nothing_observed() {
    let ctx = SessionContext::default();
    let cfg = Config::default();
    assert_eq!(agent_tracker::spawn_cost(&ctx, &cfg), cfg.agent_spawn_cost);
}

#[test]
fn spawn_cost_scales_with_observed_context() {
    // A sub-agent inherits roughly the parent's live context and re-sends it
    // every turn. The old flat 350K ignored this: on the incident session the
    // real per-request context was ~86K, and six dispatched agents were costed
    // at 2.1M against a real ~290M.
    let mut ctx = SessionContext::default();
    ctx.real_ctx_tokens = 150_000;
    let cfg = Config::default();
    let cost = agent_tracker::spawn_cost(&ctx, &cfg);
    assert!(
        cost > cfg.agent_spawn_cost,
        "expected observed context to raise the estimate above the flat constant, got {cost}"
    );
    assert_eq!(cost, 600_000);
}

#[test]
fn spawn_cost_never_reports_below_the_configured_floor() {
    // Correct upward only — a tiny observed context must not make a spawn look
    // cheaper than the compiled-in guess.
    let mut ctx = SessionContext::default();
    ctx.real_ctx_tokens = 1_000;
    let cfg = Config::default();
    assert_eq!(agent_tracker::spawn_cost(&ctx, &cfg), cfg.agent_spawn_cost);
}

// ── D9: agent budget diverged from intensity budget ────────────────────────

#[test]
fn agent_cost_warning_honors_pinned_context_window() {
    // agent_tracker hardcoded `compact_threshold_tokens * 5/4` (112,500) while
    // its comment claimed parity with intensity.rs. With a pinned 1M window the
    // two disagreed ~9x, so this tag fired from the very first spawn forever.
    let mut cfg = Config::default();
    cfg.context_window_tokens = 1_000_000;
    cfg.agent_warn_threshold_pct = 0.5; // fires at 500K under the pin

    let mut ctx = SessionContext::default();
    ctx.agent_spawns = 1;
    ctx.agent_estimated_tokens = 200_000; // over the OLD 56,250 bar, under the new one
    assert!(
        agent_tracker::agent_cost_warning(&ctx, &cfg).is_none(),
        "200K of agent cost must not warn against a pinned 1M budget"
    );

    ctx.agent_estimated_tokens = 600_000;
    assert!(
        agent_tracker::agent_cost_warning(&ctx, &cfg).is_some(),
        "600K of agent cost must warn against a pinned 1M budget"
    );
}

// ── D5: unlocked global context.json ───────────────────────────────────────

#[test]
fn update_persists_increments_under_repeated_writers() {
    // The live reproduction on 2026-08-21: an emitted "[agents: 2 calls]" tag
    // followed by agent_spawns == 0 on disk. Interleaving a stale full-object
    // save between load and save is exactly what the sub-agent hook fleet did.
    let dir = tmp_dir("update");
    for _ in 0..25 {
        SessionContext::update(&dir, |c| c.note_agent_spawn("Agent", 1_000));
    }
    let got = SessionContext::load(&dir);
    assert_eq!(
        got.agent_spawns, 25,
        "every spawn must survive the round-trip"
    );
    assert_eq!(got.agent_estimated_tokens, 25_000);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_is_not_wedged_by_an_abandoned_lock() {
    // A hook that dies mid-update leaves context.lock behind. Honoring it
    // forever would freeze every later write; the gate must fail open.
    let dir = tmp_dir("stale");
    std::fs::write(dir.join("context.lock"), b"").unwrap();
    SessionContext::update(&dir, |c| c.note_agent_spawn("Agent", 42));
    let got = SessionContext::load(&dir);
    assert_eq!(
        got.agent_spawns, 1,
        "a pre-existing lock must not silently drop the update"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn concurrent_updates_do_not_lose_every_increment() {
    // Threads stand in for the parallel hook processes. Without serialization
    // this collapses toward 1; the point is that it does not.
    let dir = tmp_dir("concurrent");
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let d = dir.clone();
            std::thread::spawn(move || {
                for _ in 0..5 {
                    SessionContext::update(&d, |c| c.note_agent_spawn("Agent", 10));
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let got = SessionContext::load(&dir);
    assert_eq!(
        got.agent_spawns, 40,
        "40 serialized increments expected, got {}",
        got.agent_spawns
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ── D4: spawns counted on return, not dispatch ─────────────────────────────

#[test]
fn track_spawn_counts_at_dispatch() {
    // PreToolUse calls this before the agent runs, so a fan-out is visible
    // while it is still in flight — the only window a burst guard can act in.
    let dir = tmp_dir("dispatch");
    for _ in 0..6 {
        squeez::commands::track::run_spawn_with_dir("Agent", &dir);
    }
    let got = SessionContext::load(&dir);
    assert_eq!(got.agent_spawns, 6);
    assert!(got.agent_estimated_tokens > 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn track_spawn_ignores_non_agent_tools() {
    let dir = tmp_dir("nonagent");
    squeez::commands::track::run_spawn_with_dir("Bash", &dir);
    squeez::commands::track::run_spawn_with_dir("WebFetch", &dir);
    assert_eq!(SessionContext::load(&dir).agent_spawns, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

// ── D3: warnings reachable only from wrap.rs (Bash-only) ───────────────────

#[test]
fn burst_warning_surfaces_without_any_bash_call() {
    // The incident ran Agent/WebFetch/WebSearch and never Bash, so the burst
    // warning — correct at 16 spawns against a threshold of 5 — had no carrier
    // and appeared zero times. compress_output runs at PostToolUse for every
    // tool, so a non-Bash result must be able to carry it.
    let dir = tmp_dir("notice");
    let cfg = Config::default();
    SessionContext::update(&dir, |c| {
        for _ in 0..cfg.parallel_agent_burst_threshold.max(5) {
            c.note_agent_spawn("Agent", 350_000);
        }
    });

    // A WebFetch result that compresses to nothing: the notice must still ride
    // out on its own, via additionalContext.
    let payload = r#"{"tool_name":"WebFetch","tool_response":{"result":"short"}}"#;
    let out = squeez::commands::compress_output::render_for_test(payload, "WebFetch", &dir, &cfg);
    assert!(
        out.contains("WORKFLOW BURST"),
        "burst warning must reach a non-Bash tool result; got: {out}"
    );
    assert!(out.contains("additionalContext"), "got: {out}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_notices_means_no_output_for_a_clean_call() {
    // Silence stays free: squeez must not start emitting a JSON blob on every
    // uneventful tool call just because the notice path exists.
    let dir = tmp_dir("quiet");
    let cfg = Config::default();
    let payload = r#"{"tool_name":"WebFetch","tool_response":{"result":"short"}}"#;
    let out = squeez::commands::compress_output::render_for_test(payload, "WebFetch", &dir, &cfg);
    assert!(out.trim().is_empty(), "expected no output, got: {out}");
    let _ = std::fs::remove_dir_all(&dir);
}
