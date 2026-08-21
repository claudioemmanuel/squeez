// Measuring a finished sub-agent instead of estimating it.
//
// Dispatch-time cost is necessarily a guess: a hook cannot know how many turns
// an agent will take. At SubagentStop it no longer has to guess — the turns have
// happened and the agent's transcript is on disk.

use squeez::context::cache::SessionContext;
use squeez::context::transcript::{measure_agent_usage_in, AgentUsage};

const TURN: &str = r#"{"type":"assistant","message":{"usage":{"input_tokens":10,"cache_creation_input_tokens":100,"cache_read_input_tokens":50000,"output_tokens":200}}}"#;

#[test]
fn sums_every_request_not_just_the_last() {
    // A sub-agent's cost is the whole conversation it held. Reading only the
    // final turn (what last_context_tokens does, correctly, for a different
    // purpose) would report one turn's context as the agent's entire cost.
    let text = format!("{TURN}\n{TURN}\n{TURN}");
    let u = measure_agent_usage_in(&text).expect("usage found");
    assert_eq!(u.requests, 3);
    assert_eq!(u.cache_read, 150_000);
    assert_eq!(u.output, 600);
    assert_eq!(u.total(), 3 * (10 + 100 + 50_000 + 200));
}

#[test]
fn cache_read_is_counted() {
    // The whole point. The host-reported `subagent_tokens` for two real agents
    // was ~100K and ~120K; their transcripts showed 4.08M and 5.35M, dominated
    // by cache_read. Any total that omits it understates by ~40x.
    let u = measure_agent_usage_in(TURN).expect("usage");
    assert!(
        u.total() > u.output * 100,
        "cache_read must dominate the total, got {}",
        u.total()
    );
}

#[test]
fn does_not_confuse_input_tokens_with_cache_read_input_tokens() {
    // "input_tokens" is a substring of "cache_read_input_tokens"; a sloppy key
    // match would double-count or mis-assign.
    let u = measure_agent_usage_in(TURN).expect("usage");
    assert_eq!(u.input, 10);
    assert_eq!(u.cache_read, 50_000);
}

#[test]
fn absent_usage_yields_none_so_the_estimate_survives() {
    // A missing measurement must leave the dispatch estimate standing, never
    // overwrite it with zero.
    assert!(measure_agent_usage_in("").is_none());
    assert!(measure_agent_usage_in(r#"{"type":"user","message":{}}"#).is_none());
    assert!(measure_agent_usage_in(r#"{"usage":{"input_tokens":0,"output_tokens":0}}"#).is_none());
}

// ── Reconciliation ─────────────────────────────────────────────────────────

#[test]
fn falls_back_to_the_estimate_until_something_finishes() {
    let mut c = SessionContext::default();
    c.note_agent_spawn("Agent", 350_000);
    c.note_agent_spawn("Agent", 350_000);
    assert_eq!(c.effective_agent_tokens(), 700_000);
}

#[test]
fn measured_siblings_price_the_ones_still_running() {
    // Once one agent's real cost is known, it predicts its siblings far better
    // than a compiled-in constant does.
    let mut c = SessionContext::default();
    for _ in 0..4 {
        c.note_agent_spawn("Agent", 350_000);
    }
    c.note_agent_measured(4_000_000); // one finished, and it cost 4M, not 350K
    // 4M measured + 3 in flight priced at the observed 4M each.
    assert_eq!(c.effective_agent_tokens(), 16_000_000);
    assert!(
        c.effective_agent_tokens() > c.agent_estimated_tokens * 10,
        "measurement must be allowed to overrule the estimate upward"
    );
}

#[test]
fn measured_values_survive_the_json_round_trip() {
    // The last_burst_tag bug: a field present in the struct and the Default but
    // absent from to_json/from_json reverts silently on every load.
    let dir = std::env::temp_dir().join(format!("squeez_measure_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    SessionContext::update(&dir, |c| {
        c.note_agent_spawn("Agent", 350_000);
        c.note_agent_measured(4_076_214);
    });
    let got = SessionContext::load(&dir);
    assert_eq!(got.agent_measured_tokens, 4_076_214);
    assert_eq!(got.agent_measured_count, 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn usage_total_saturates_rather_than_overflowing() {
    let u = AgentUsage {
        requests: 1,
        input: u64::MAX,
        cache_creation: u64::MAX,
        cache_read: u64::MAX,
        output: u64::MAX,
    };
    assert_eq!(u.total(), u64::MAX);
}
