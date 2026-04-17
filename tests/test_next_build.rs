use squeez::commands::{next_build::NextBuildHandler, Handler};
use squeez::config::Config;

#[test]
fn drops_telemetry_and_progress_keeps_route_table() {
    let lines = vec![
        "▲ Next.js 15.1.2".to_string(),
        "Creating an optimized production build ...".to_string(),
        "Compiled successfully".to_string(),
        "Collecting page data ...".to_string(),
        "Generating static pages (0/240)".to_string(),
        "(12/240)".to_string(),
        "(240/240)".to_string(),
        "Route (app)                              Size  First Load JS".to_string(),
        "┌ ○ /                                  1.2 kB        90 kB".to_string(),
        "├ ○ /dashboard                         3.4 kB       120 kB".to_string(),
        "└ ƒ /api/health                          0 B          0 B".to_string(),
        "Attention: Next.js now collects completely anonymous telemetry".to_string(),
        "https://nextjs.org/telemetry".to_string(),
    ];
    let result = NextBuildHandler.compress("next build", lines, &Config::default());
    assert!(!result.iter().any(|l| l.starts_with("▲ Next.js")));
    assert!(!result.iter().any(|l| l.starts_with("Creating an optimized")));
    assert!(!result.iter().any(|l| l.starts_with("(12/240)")));
    assert!(!result.iter().any(|l| l.contains("nextjs.org/telemetry")));
    assert!(result.iter().any(|l| l.contains("/dashboard")));
    assert!(result.iter().any(|l| l.contains("/api/health")));
}

#[test]
fn keeps_errors() {
    let lines = vec![
        "▲ Next.js 15.1.2".to_string(),
        "Failed to compile.".to_string(),
        "./app/page.tsx:12:5".to_string(),
        "Type error: Property 'foo' does not exist on type 'Bar'.".to_string(),
    ];
    let result = NextBuildHandler.compress("next build", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("Failed to compile")));
    assert!(result.iter().any(|l| l.contains("Type error")));
}
