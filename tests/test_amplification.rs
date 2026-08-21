// Amplification-aware intensity.
//
// A tool result is re-sent as cache_read on every later request until
// compaction, so its true cost is `size × (1 + future turns)`. Measured on the
// 2026-08-21 session: 1.1M tokens of unique tool output produced 290.1M of
// cache_read — a 27x multiple that squeez ignored, compressing call #3 and call
// #110 identically.
//
// Most of these tests assert SILENCE. Escalating costs fidelity, so the
// mechanism must stay out of the way except on the shape it is built for: a
// long session, with headroom left, producing output big enough to be worth
// compressing.

use squeez::config::Config;
use squeez::context::cache::{BurnEntry, SessionContext};
use squeez::context::intensity::{amplification_estimate, amplification_level, Intensity};

/// A session that SHOULD escalate: long, roomy, and producing real output.
fn amplifying_ctx() -> SessionContext {
    let mut c = SessionContext::default();
    c.call_counter = 60;
    c.real_ctx_window = 200_000;
    c.real_ctx_tokens = 40_000; // plenty of headroom left
    for _ in 0..8 {
        c.burn_window.push(BurnEntry {
            tokens: 2_000,
            ts: 0,
            call_n: 0,
        });
    }
    c
}

#[test]
fn escalates_on_a_long_session_with_headroom_and_real_output() {
    let cfg = Config::default();
    assert_eq!(amplification_level(&amplifying_ctx(), &cfg), Intensity::Ultra);
}

#[test]
fn reports_the_re_read_multiple_it_is_acting_on() {
    // The header shows this number; an unexplained jump to Ultra reads as a
    // malfunction, and the multiple is the entire argument for escalating.
    let cfg = Config::default();
    let n = amplification_estimate(&amplifying_ctx(), &cfg).expect("estimate available");
    assert!(n > 0, "expected a positive re-read estimate, got {n}");
}

// ── The silences ───────────────────────────────────────────────────────────

#[test]
fn stays_quiet_on_a_short_session() {
    // THE point of not keying this on "early calls". At call #3 a 5-call
    // session and a 500-call session are indistinguishable; compressing the
    // short one hard costs fidelity to save nothing.
    let cfg = Config::default();
    let mut c = amplifying_ctx();
    c.call_counter = 3;
    assert_eq!(amplification_level(&c, &cfg), Intensity::Lite);
}

#[test]
fn stays_quiet_when_compaction_is_imminent() {
    // Content added just before compaction is carried a few turns and then
    // dropped — there is nothing to amortise aggression over.
    let cfg = Config::default();
    let mut c = amplifying_ctx();
    c.real_ctx_tokens = 199_000; // essentially no headroom
    assert_eq!(amplification_level(&c, &cfg), Intensity::Lite);
}

#[test]
fn stays_quiet_when_output_is_trivially_small() {
    // 27x of nearly nothing is still nothing. The measured light session moved
    // 28.6K of tool output all told — 0.34% of its own cost. Squeezing that
    // harder buys nothing and costs fidelity.
    let cfg = Config::default();
    let mut c = amplifying_ctx();
    c.burn_window.clear();
    for _ in 0..8 {
        c.burn_window.push(BurnEntry {
            tokens: 40,
            ts: 0,
            call_n: 0,
        });
    }
    assert_eq!(amplification_level(&c, &cfg), Intensity::Lite);
}

#[test]
fn stays_quiet_without_enough_data_to_judge() {
    // A burn window too short to have a meaningful median is not evidence of
    // anything; guessing from it would be the same mistake as the flat
    // agent_spawn_cost constant.
    let cfg = Config::default();
    let mut c = amplifying_ctx();
    c.burn_window.clear();
    assert_eq!(amplification_level(&c, &cfg), Intensity::Lite);
}

#[test]
fn respects_the_off_switch() {
    let mut cfg = Config::default();
    cfg.amplification_aware = false;
    assert_eq!(amplification_level(&amplifying_ctx(), &cfg), Intensity::Lite);
}

// ── Composition with the pre-existing survival trigger ─────────────────────

#[test]
fn survival_and_amplification_are_combined_not_replaced() {
    // They fire at OPPOSITE ends of a session: survival when the context is
    // nearly full, amplification when there is room left. Taking the stronger
    // is what makes them complementary; picking one would re-break the other.
    assert_eq!(
        Intensity::strongest(Intensity::Lite, Intensity::Ultra),
        Intensity::Ultra
    );
    assert_eq!(
        Intensity::strongest(Intensity::Ultra, Intensity::Full),
        Intensity::Ultra
    );
    assert_eq!(
        Intensity::strongest(Intensity::Full, Intensity::Lite),
        Intensity::Full
    );
    assert!(Intensity::Ultra.rank() > Intensity::Full.rank());
    assert!(Intensity::Full.rank() > Intensity::Lite.rank());
}

#[test]
fn escalation_actually_tightens_the_line_budget() {
    // The tier must reach the knobs that do the work, not just the label.
    let cfg = Config::default();
    let full = squeez::context::intensity::scale(&cfg, Intensity::Full);
    let ultra = squeez::context::intensity::scale(&cfg, Intensity::Ultra);
    assert!(
        ultra.max_lines < full.max_lines,
        "Ultra must truncate harder than Full ({} vs {})",
        ultra.max_lines,
        full.max_lines
    );
}

// ── Calibration, against the measured incident ─────────────────────────────

#[test]
fn fires_on_the_session_shape_that_caused_the_incident() {
    // The research sub-agents: ~112 calls in, ~5K tokens/call, ~90K of a 200K
    // window used — so ~22 calls of headroom. An earlier draft used a headroom
    // floor of 25 and this case came out QUIET, which would have shipped a
    // mechanism that misses the only thing it was built for. 22 re-reads of a
    // 5K result is ~55K tokens per agent, across 26 agents.
    let cfg = Config::default();
    let mut c = SessionContext::default();
    c.call_counter = 112;
    c.real_ctx_window = 200_000;
    c.real_ctx_tokens = 90_000;
    for _ in 0..8 {
        c.burn_window.push(BurnEntry {
            tokens: 5_000,
            ts: 0,
            call_n: 0,
        });
    }
    assert_eq!(
        amplification_level(&c, &cfg),
        Intensity::Ultra,
        "the research sub-agent profile must escalate — it is the incident"
    );
}
