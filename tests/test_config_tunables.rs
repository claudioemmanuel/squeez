/// Tests for phase 5: configurable tunables parsed from config.ini.
use squeez::config::Config;

#[test]
fn defaults_match_prior_hardcoded_values() {
    let c = Config::default();
    assert_eq!(c.max_call_log, 32);
    assert_eq!(c.recent_window, 16);
    assert!((c.similarity_threshold - 0.85).abs() < 1e-6);
    assert!((c.ultra_trigger_pct - 0.65).abs() < 1e-6);
    assert_eq!(c.mcp_prior_summaries_default, 5);
    assert_eq!(c.mcp_recent_calls_default, 10);
}

#[test]
fn max_call_log_parses_from_ini() {
    let c = Config::from_str("max_call_log = 64");
    assert_eq!(c.max_call_log, 64);
}

#[test]
fn recent_window_parses_from_ini() {
    let c = Config::from_str("recent_window = 8");
    assert_eq!(c.recent_window, 8);
}

#[test]
fn similarity_threshold_parses_from_ini() {
    let c = Config::from_str("similarity_threshold = 0.90");
    assert!((c.similarity_threshold - 0.90).abs() < 1e-5);
}

#[test]
fn ultra_trigger_pct_parses_from_ini() {
    let c = Config::from_str("ultra_trigger_pct = 0.70");
    assert!((c.ultra_trigger_pct - 0.70).abs() < 1e-5);
}

#[test]
fn wrap_timeout_defaults_to_the_prior_hardcoded_120s() {
    assert_eq!(Config::default().wrap_timeout_secs, 120);
}

#[test]
fn wrap_timeout_parses_from_ini() {
    assert_eq!(Config::from_str("wrap_timeout_secs = 1800").wrap_timeout_secs, 1800);
}

#[test]
fn wrap_timeout_env_overrides_config() {
    // The point of the env knob (#197): unblock one long build without editing
    // a file and without rebuilding the binary.
    assert_eq!(squeez::config::resolve_wrap_timeout_secs(120, Some("3600")), 3600);
}

#[test]
fn wrap_timeout_falls_back_when_env_is_unusable() {
    for env in [None, Some(""), Some("nope"), Some("-5"), Some("0")] {
        assert_eq!(
            squeez::config::resolve_wrap_timeout_secs(600, env),
            600,
            "env {env:?} must not override a valid config value"
        );
    }
}

#[test]
fn wrap_timeout_zero_in_config_keeps_the_default() {
    // A `wrap_timeout_secs = 0` typo would otherwise kill every command
    // instantly; treat it as unset rather than as "no time at all".
    assert_eq!(squeez::config::resolve_wrap_timeout_secs(0, None), 120);
}

#[test]
fn mcp_defaults_parse_from_ini() {
    let ini = "mcp_prior_summaries_default = 10\nmcp_recent_calls_default = 20";
    let c = Config::from_str(ini);
    assert_eq!(c.mcp_prior_summaries_default, 10);
    assert_eq!(c.mcp_recent_calls_default, 20);
}

#[test]
fn invalid_values_fall_back_to_defaults() {
    let c = Config::from_str("max_call_log = not_a_number\nsimilarity_threshold = bad");
    assert_eq!(c.max_call_log, 32);
    assert!((c.similarity_threshold - 0.85).abs() < 1e-6);
}

#[test]
fn class_density_and_net_win_defaults() {
    let c = Config::default();
    assert!(c.class_density);
    assert_eq!(c.net_win_min_tokens, 24);
}

#[test]
fn class_density_parses_from_ini() {
    let c = Config::from_str("class_density = false");
    assert!(!c.class_density);
}

#[test]
fn net_win_min_tokens_parses_from_ini() {
    let c = Config::from_str("net_win_min_tokens = 0");
    assert_eq!(c.net_win_min_tokens, 0);
    let c = Config::from_str("net_win_min_tokens = 40");
    assert_eq!(c.net_win_min_tokens, 40);
}

#[test]
fn tunables_propagate_to_session_context() {
    use squeez::context::cache::SessionContext;
    let mut cfg = Config::default();
    cfg.max_call_log = 8;
    cfg.recent_window = 4;
    cfg.similarity_threshold = 0.95;
    let mut ctx = SessionContext::default();
    ctx.init_tunables_from_config(&cfg);
    assert_eq!(ctx.max_call_log, 8);
    assert_eq!(ctx.recent_window, 4);
    assert!((ctx.similarity_threshold - 0.95).abs() < 1e-6);
}

#[test]
fn custom_max_call_log_enforced_in_recording() {
    use squeez::context::cache::SessionContext;
    let mut cfg = Config::default();
    cfg.max_call_log = 5;
    let mut ctx = SessionContext::default();
    ctx.init_tunables_from_config(&cfg);
    // Record 10 calls — should only keep last 5.
    for i in 0..10u64 {
        let n = ctx.next_call_n();
        ctx.record_call(&format!("cmd{}", i), i, i as usize, n);
    }
    assert_eq!(ctx.call_log.len(), 5);
}
