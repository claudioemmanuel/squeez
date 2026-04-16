use squeez::config::Config;
use squeez::context::intensity::{budget, derive, scale, Intensity};

fn cfg() -> Config {
    Config::default()
}

#[test]
fn adaptive_on_zero_usage_is_full() {
    // Empty session → gentler Full compression (was always-Ultra previously).
    assert_eq!(derive(0, &cfg()), Intensity::Full);
}

#[test]
fn adaptive_on_half_budget_is_full() {
    let c = cfg();
    assert_eq!(derive(budget(&c) / 2, &c), Intensity::Full);
}

#[test]
fn adaptive_on_seventy_nine_percent_is_full() {
    let c = cfg();
    // 60% is below the 65% threshold — still Full
    assert_eq!(derive(budget(&c) * 60 / 100, &c), Intensity::Full);
}

#[test]
fn adaptive_on_eighty_percent_is_ultra() {
    let c = cfg();
    assert_eq!(derive(budget(&c) * 80 / 100, &c), Intensity::Ultra);
}

#[test]
fn adaptive_on_full_budget_is_ultra() {
    let c = cfg();
    assert_eq!(derive(budget(&c), &c), Intensity::Ultra);
}

#[test]
fn adaptive_on_overbudget_is_ultra() {
    let c = cfg();
    assert_eq!(derive(budget(&c) * 5, &c), Intensity::Ultra);
}

#[test]
fn adaptive_off_zero_usage_is_lite() {
    let mut c = cfg();
    c.adaptive_intensity = false;
    assert_eq!(derive(0, &c), Intensity::Lite);
}

#[test]
fn adaptive_off_overbudget_still_lite() {
    let mut c = cfg();
    c.adaptive_intensity = false;
    assert_eq!(derive(budget(&c) * 5, &c), Intensity::Lite);
}

#[test]
fn scale_lite_passthrough() {
    let c = cfg();
    let s = scale(&c, Intensity::Lite);
    assert_eq!(s.max_lines, c.max_lines);
    assert_eq!(s.git_diff_max_lines, c.git_diff_max_lines);
    assert_eq!(s.dedup_min, c.dedup_min);
}

#[test]
fn scale_ultra_smaller_than_full() {
    let c = cfg();
    let f = scale(&c, Intensity::Full);
    let u = scale(&c, Intensity::Ultra);
    assert!(u.max_lines <= f.max_lines);
    assert!(u.docker_logs_max_lines <= f.docker_logs_max_lines);
}

#[test]
fn floors_prevent_zero() {
    let mut c = cfg();
    c.max_lines = 1;
    c.git_diff_max_lines = 1;
    c.dedup_min = 0;
    let u = scale(&c, Intensity::Ultra);
    assert!(u.max_lines >= 20);
    assert!(u.git_diff_max_lines >= 20);
    assert!(u.dedup_min >= 2);
}
