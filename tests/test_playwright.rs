use squeez::commands::{playwright::PlaywrightHandler, Handler};
use squeez::config::Config;

#[test]
fn drops_passing_keeps_failures_and_summary() {
    let lines = vec![
        "Running 3 tests using 3 workers".to_string(),
        "  ✓  1 [chromium] › tests/login.spec.ts:5:5 › should login (234ms)".to_string(),
        "  ✓  2 [firefox] › tests/login.spec.ts:5:5 › should login (312ms)".to_string(),
        "  ✗  3 [webkit] › tests/auth.spec.ts:20:5 › should redirect (567ms)".to_string(),
        "    Error: expect(received).toBe(expected)".to_string(),
        "    Expected: 302, Received: 200".to_string(),
        "  3 tests ran".to_string(),
        "    1 failed".to_string(),
        "    2 passed (2.3s)".to_string(),
    ];
    let result = PlaywrightHandler.compress("playwright test", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains('✗') || l.contains("fail")));
    assert!(result.iter().any(|l| l.contains("Error")));
    assert!(result.iter().any(|l| l.contains("3 tests ran") || l.contains("failed")));
    // Passing test lines should be gone
    assert!(!result.iter().any(|l| l.contains("should login") && l.contains('✓')));
}

#[test]
fn all_passing_keeps_summary() {
    let lines = vec![
        "  ✓  1 [chromium] › tests/home.spec.ts:3:1 › loads homepage (100ms)".to_string(),
        "  ✓  2 [chromium] › tests/home.spec.ts:8:1 › has title (80ms)".to_string(),
        "  2 passed (1.2s)".to_string(),
    ];
    let result = PlaywrightHandler.compress("playwright test", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("passed")));
    // Individual passing test lines should be filtered out
    assert!(!result.iter().any(|l| l.contains('✓') && l.contains("homepage")));
}

#[test]
fn drops_screenshot_lines_for_passing() {
    let lines = vec![
        "  ✓  1 [chromium] › tests/home.spec.ts:3:1 › ok (100ms)".to_string(),
        "    screenshot: tests/home-chromium/screenshot.png".to_string(),
        "  2 passed (1.2s)".to_string(),
    ];
    let result = PlaywrightHandler.compress("playwright test", lines, &Config::default());
    assert!(!result.iter().any(|l| l.contains("screenshot") && l.contains(".png")));
}
