use squeez::commands::{test_runner::TestRunnerHandler, Handler};
use squeez::config::Config;

#[test]
fn keeps_failures_drops_passing() {
    let mut lines: Vec<String> = (0..50)
        .map(|i| format!("  \u{2713} test_{} (2ms)", i))
        .collect();
    lines.push("  \u{2717} test_auth: expected 200 got 401".to_string());
    lines.push("Tests: 1 failed, 50 passed".to_string());
    let result = TestRunnerHandler.compress("jest", lines, &Config::default());
    assert!(result
        .iter()
        .any(|l| l.contains("\u{2717}") || l.contains("failed")));
    assert!(result
        .iter()
        .any(|l| l.contains("50 passed") || l.contains("Tests:")));
    assert!(result.len() < 10);
}

#[test]
fn all_passing_still_shows_summary() {
    let mut lines: Vec<String> = (0..20).map(|i| format!("  \u{2713} test_{}", i)).collect();
    lines.push("Tests: 20 passed".to_string());
    let result = TestRunnerHandler.compress("jest", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("20 passed")));
}
