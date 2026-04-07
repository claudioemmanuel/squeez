use squeez::config::Config;
use squeez::context::summarize::{apply, should_apply};

fn cfg(threshold: usize) -> Config {
    let mut c = Config::default();
    c.summarize_threshold_lines = threshold;
    c
}

#[test]
fn should_apply_under_threshold_false() {
    let c = cfg(100);
    let lines: Vec<String> = (0..50).map(|i| format!("l{}", i)).collect();
    assert!(!should_apply(&lines, &c));
}

#[test]
fn should_apply_over_threshold_true() {
    let c = cfg(100);
    let lines: Vec<String> = (0..200).map(|i| format!("l{}", i)).collect();
    assert!(should_apply(&lines, &c));
}

#[test]
fn summary_caps_total_output() {
    let lines: Vec<String> = (0..5000).map(|i| format!("line {}", i)).collect();
    let out = apply(lines, "cargo build");
    assert!(out.len() <= 40, "summary too long: {} lines", out.len());
}

#[test]
fn summary_preserves_tail() {
    let lines: Vec<String> = (0..1000).map(|i| format!("L{}", i)).collect();
    let out = apply(lines, "x");
    assert!(out.contains(&"L999".to_string()));
    assert!(out.contains(&"L990".to_string()));
}

#[test]
fn summary_drops_head() {
    let lines: Vec<String> = (0..1000).map(|i| format!("L{}", i)).collect();
    let out = apply(lines, "x");
    assert!(!out.contains(&"L0".to_string()));
    assert!(!out.contains(&"L500".to_string()));
}

#[test]
fn summary_extracts_top_errors() {
    let mut lines: Vec<String> = (0..600).map(|i| format!("noise {}", i)).collect();
    lines.push("error: type mismatch on line 42".to_string());
    let out = apply(lines, "cargo check");
    let joined = out.join("\n");
    assert!(joined.contains("top_errors"));
    assert!(joined.contains("type mismatch"));
}

#[test]
fn summary_includes_total_lines() {
    let lines: Vec<String> = (0..777).map(|_| "x".to_string()).collect();
    let out = apply(lines, "cmd");
    assert!(out.iter().any(|l| l == "total_lines=777"));
}

#[test]
fn summary_starts_with_header() {
    let lines: Vec<String> = (0..600).map(|_| "x".to_string()).collect();
    let out = apply(lines, "git diff HEAD");
    assert!(out[0].starts_with("squeez:summary cmd="));
    assert!(out[0].contains("git diff HEAD"));
}
