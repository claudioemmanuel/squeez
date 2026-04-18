use squeez::config::Config;
use squeez::filter::compress;

// These tests exercise the dispatcher in src/filter.rs for commands
// common in a Next.js + Cloudflare + Bun + Playwright stack.

#[test]
fn wrangler_deploy_goes_to_wrangler_handler() {
    let lines = vec![
        "Total Upload: 100 KiB / gzip: 40 KiB".to_string(),
        "  https://x.workers.dev".to_string(),
    ];
    let out = compress("wrangler deploy", lines, &Config::default());
    // WranglerHandler drops "Total Upload:" prefix.
    assert!(!out.iter().any(|l| l.starts_with("Total Upload:")));
    assert!(out.iter().any(|l| l.contains("workers.dev")));
}

#[test]
fn bunx_wrangler_is_unwrapped_then_dispatched() {
    let lines = vec!["Total Upload: 1 KiB / gzip: 1 KiB".to_string()];
    let out = compress("bunx wrangler deploy", lines, &Config::default());
    assert!(!out.iter().any(|l| l.starts_with("Total Upload:")));
}

#[test]
fn bun_test_routes_to_test_runner() {
    let mut lines: Vec<String> = (0..30).map(|i| format!("  ✓  test_{i}")).collect();
    lines.push("Tests: 30 passed".to_string());
    let out = compress("bun test", lines, &Config::default());
    // TestRunnerHandler drops passing lines.
    let pass = out.iter().filter(|l| l.contains("✓")).count();
    assert!(pass < 5, "expected test-runner dedup, kept {pass} passing");
    assert!(out.iter().any(|l| l.contains("30 passed")));
}

#[test]
fn bun_run_dev_stays_on_package_mgr() {
    let lines = vec!["$ next dev".to_string(), "ready on http://localhost:3000".to_string()];
    let out = compress("bun run dev", lines, &Config::default());
    // PackageMgrHandler just dedups/truncates; content should survive.
    assert!(out.iter().any(|l| l.contains("localhost:3000")));
}

#[test]
fn playwright_via_bunx_routes_correctly() {
    let mut lines: Vec<String> = (0..40)
        .map(|i| format!("  ✓  {i} [chromium] › foo.spec.ts › renders"))
        .collect();
    lines.push("40 passed (10s)".to_string());
    let out = compress("bunx playwright test", lines, &Config::default());
    let pass = out.iter().filter(|l| l.contains("✓")).count();
    assert!(pass < 10, "playwright handler should drop passing lines, kept {pass}");
}

#[test]
fn drizzle_kit_routes_to_database() {
    let lines = vec![
        "+ CREATE TABLE users (id INT)".to_string(),
        "drizzle-kit: v0.20.0".to_string(),
        "[✓] Changes applied".to_string(),
    ];
    let out = compress("drizzle-kit push", lines, &Config::default());
    // DatabaseHandler drops lines starting with '+'.
    assert!(!out.iter().any(|l| l.starts_with('+')));
    assert!(out.iter().any(|l| l.contains("Changes applied")));
}

#[test]
fn next_build_routes_to_next_build_handler() {
    let lines = vec![
        "▲ Next.js 15.1.2".to_string(),
        "Compiled successfully".to_string(),
        "┌ ○ /                 1.2 kB    90 kB".to_string(),
    ];
    let out = compress("next build", lines, &Config::default());
    assert!(!out.iter().any(|l| l.starts_with("▲ Next.js")));
    assert!(out.iter().any(|l| l.contains("/")));
}
