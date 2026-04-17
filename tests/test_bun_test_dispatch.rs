use squeez::filter;
use squeez::config::Config;

#[test]
fn bun_test_routes_to_test_runner_keeps_failures() {
    let mut lines: Vec<String> = (0..30)
        .map(|i| format!("✓ utils > helper_{} (1ms)", i))
        .collect();
    lines.push("✗ auth > login [0.12ms]".to_string());
    lines.push("  error: expect(received).toBe(expected)".to_string());
    lines.push(" 31 tests 1 fail 30 pass".to_string());

    let result = filter::compress("bun test", lines, &Config::default());
    // Failures and summary must be present
    assert!(result.iter().any(|l| l.contains('✗') || l.contains("fail")));
    assert!(result.iter().any(|l| l.contains("31 tests") || l.contains("1 fail")));
    // Passing tests should be suppressed (output much shorter than 30 lines)
    assert!(result.len() < 10);
}

#[test]
fn bun_install_routes_to_package_mgr() {
    let lines = vec![
        "bun install v1.1.0".to_string(),
        "Resolving dependencies".to_string(),
        "✓ 120 packages installed [1.23s]".to_string(),
    ];
    // Package manager handler: smart_filter + dedup + truncation — just verify no panic
    let result = filter::compress("bun install", lines, &Config::default());
    assert!(!result.is_empty());
}
