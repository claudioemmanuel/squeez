use squeez::config::Config;
use squeez::context::intensity::{budget, derive, scale, Intensity};

fn cfg() -> Config {
    Config::default()
}

#[test]
fn boundary_zero_pct_lite() {
    assert_eq!(derive(0, &cfg()), Intensity::Lite);
}

#[test]
fn boundary_49_pct_lite() {
    let c = cfg();
    assert_eq!(derive(budget(&c) * 49 / 100, &c), Intensity::Lite);
}

#[test]
fn boundary_50_pct_full() {
    let c = cfg();
    assert_eq!(derive(budget(&c) * 50 / 100, &c), Intensity::Full);
}

#[test]
fn boundary_79_pct_full() {
    let c = cfg();
    assert_eq!(derive(budget(&c) * 79 / 100, &c), Intensity::Full);
}

#[test]
fn boundary_80_pct_ultra() {
    let c = cfg();
    assert_eq!(derive(budget(&c) * 80 / 100, &c), Intensity::Ultra);
}

#[test]
fn over_budget_still_ultra() {
    let c = cfg();
    assert_eq!(derive(budget(&c) * 5, &c), Intensity::Ultra);
}

#[test]
fn adaptive_disabled_always_lite_even_overbudget() {
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
