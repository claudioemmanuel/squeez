use squeez::commands::{playwright::PlaywrightHandler, Handler};
use squeez::config::Config;

#[test]
fn drops_passing_keeps_failures_and_summary() {
    let mut lines: Vec<String> = (0..80)
        .map(|i| format!("  ✓  {} [chromium] › foo.spec.ts:{}:3 › renders ({i}ms)", i, i))
        .collect();
    lines.push("  ✘  81 [chromium] › auth.spec.ts:10:3 › login fails (1.2s)".to_string());
    lines.push("    Error: expect(locator).toBeVisible() failed".to_string());
    lines.push("    at auth.spec.ts:14:32".to_string());
    lines.push("  1 failed".to_string());
    lines.push("  80 passed (22.4s)".to_string());
    let result = PlaywrightHandler.compress("playwright test", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("✘") || l.contains("failed")));
    assert!(result.iter().any(|l| l.contains("80 passed")));
    assert!(result.iter().any(|l| l.contains("auth.spec.ts")));
    // passing lines should be heavily trimmed
    let pass_count = result.iter().filter(|l| l.contains("✓")).count();
    assert!(pass_count < 10, "too many passing lines kept: {pass_count}");
}

#[test]
fn drops_trace_artefact_paths() {
    let lines = vec![
        "  ✘  1 [chromium] › a.spec.ts › foo".to_string(),
        "attachment #1: trace (application/zip) ─────────────".to_string(),
        "test-results/a-foo/trace.zip".to_string(),
        "To open last HTML report run: npx playwright show-report".to_string(),
        "1 failed (3.4s)".to_string(),
    ];
    let result = PlaywrightHandler.compress("playwright test", lines, &Config::default());
    assert!(!result.iter().any(|l| l.starts_with("attachment #")));
    assert!(!result.iter().any(|l| l.contains("show-report")));
    assert!(result.iter().any(|l| l.contains("1 failed")));
}
