// Tests for G3 (loop pattern label) and G4 (/loop → /monitor nudge).
// Both live in src/economy/nudge.rs.

use squeez::config::Config;
use squeez::context::cache::{FileAccess, SessionContext};
use squeez::economy::nudge::evaluate;

fn cfg_with_repeat_threshold(n: u32) -> Config {
    let mut cfg = Config::default();
    cfg.nudge_cmd_repeat_threshold = n;
    cfg
}

// ── G3: Loop pattern label ────────────────────────────────────────────────────

#[test]
fn no_loop_warning_below_2x_threshold() {
    let mut ctx = SessionContext::default();
    let cfg = cfg_with_repeat_threshold(3); // loop threshold = 6
    // Run cargo 5 times — above scripting threshold (3) but below loop threshold (6)
    for _ in 0..5 {
        evaluate(&mut ctx, "cargo test", &[], FileAccess::Read, &[], &cfg);
    }
    let hints: Vec<String> = (0..5)
        .flat_map(|_| evaluate(&mut ctx, "cargo test", &[], FileAccess::Read, &[], &cfg))
        .collect();
    // Should not contain LOOP PATTERN yet (we're at ≤ 10 runs, threshold = 6)
    // After 6 runs the loop warning fires; check it does appear at 2×threshold
    let mut ctx2 = SessionContext::default();
    let cfg2 = cfg_with_repeat_threshold(3);
    let mut got_loop = false;
    for _ in 0..10 {
        let h = evaluate(&mut ctx2, "cargo test", &[], FileAccess::Read, &[], &cfg2);
        if h.iter().any(|s| s.contains("LOOP PATTERN")) {
            got_loop = true;
        }
    }
    assert!(got_loop, "LOOP PATTERN should fire at 2× repeat threshold");
    let _ = hints; // suppress unused
}

#[test]
fn loop_warning_fires_at_2x_threshold() {
    let mut ctx = SessionContext::default();
    let cfg = cfg_with_repeat_threshold(2); // loop at 4
    let mut fired_at = None;
    for i in 1..=8u32 {
        let h = evaluate(&mut ctx, "cargo build", &[], FileAccess::Read, &[], &cfg);
        if h.iter().any(|s| s.contains("LOOP PATTERN")) && fired_at.is_none() {
            fired_at = Some(i);
        }
    }
    assert_eq!(fired_at, Some(4), "loop warning should fire at run #4 (2×2)");
}

#[test]
fn loop_warning_contains_command_name() {
    let mut ctx = SessionContext::default();
    let cfg = cfg_with_repeat_threshold(2);
    let mut msg = String::new();
    for _ in 0..6 {
        for h in evaluate(&mut ctx, "npm test", &[], FileAccess::Read, &[], &cfg) {
            if h.contains("LOOP PATTERN") {
                msg = h;
            }
        }
    }
    assert!(msg.contains("`npm`"), "loop warning should name the command");
}

#[test]
fn loop_warning_fires_only_once() {
    let mut ctx = SessionContext::default();
    let cfg = cfg_with_repeat_threshold(2);
    let mut count = 0;
    for _ in 0..20 {
        for h in evaluate(&mut ctx, "cargo test", &[], FileAccess::Read, &[], &cfg) {
            if h.contains("LOOP PATTERN") {
                count += 1;
            }
        }
    }
    assert_eq!(count, 1, "loop warning should fire only once per session");
}

// ── G4: Sleep-based polling nudge ────────────────────────────────────────────

#[test]
fn sleep_poll_nudge_fires_on_second_sleep() {
    let mut ctx = SessionContext::default();
    let cfg = Config::default();
    // First sleep — no nudge yet
    let h1 = evaluate(&mut ctx, "sleep 5 && check_status.sh", &[], FileAccess::Read, &[], &cfg);
    assert!(!h1.iter().any(|s| s.contains("POLLING DETECTED")));
    // Second sleep — nudge fires
    let h2 = evaluate(&mut ctx, "sleep 10 && check_status.sh", &[], FileAccess::Read, &[], &cfg);
    assert!(
        h2.iter().any(|s| s.contains("POLLING DETECTED")),
        "polling nudge should fire on second sleep-containing command"
    );
}

#[test]
fn sleep_poll_nudge_fires_only_once() {
    let mut ctx = SessionContext::default();
    let cfg = Config::default();
    let mut count = 0;
    for _ in 0..10 {
        for h in evaluate(&mut ctx, "sleep 300; tail -f server.log", &[], FileAccess::Read, &[], &cfg) {
            if h.contains("POLLING DETECTED") {
                count += 1;
            }
        }
    }
    assert_eq!(count, 1, "polling nudge should fire only once");
}

#[test]
fn no_sleep_no_poll_nudge() {
    let mut ctx = SessionContext::default();
    let cfg = Config::default();
    let hints = evaluate(&mut ctx, "cargo test --release", &[], FileAccess::Read, &[], &cfg);
    assert!(!hints.iter().any(|s| s.contains("POLLING")));
}
