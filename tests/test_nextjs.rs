use squeez::commands::{nextjs::NextjsHandler, Handler};
use squeez::config::Config;

#[test]
fn drops_version_banner_keeps_ready() {
    let lines = vec![
        "  ▲ Next.js 14.2.5".to_string(),
        "  - Local:        http://localhost:3000".to_string(),
        "  - Environments: .env".to_string(),
        "  - Network:      http://192.168.1.5:3000".to_string(),
        " ✓ Ready in 2.3s".to_string(),
    ];
    let result = NextjsHandler.compress("next dev", lines, &Config::default());
    assert!(!result.iter().any(|l| l.contains("Next.js 14")));
    assert!(!result.iter().any(|l| l.contains("Environments:")));
    assert!(!result.iter().any(|l| l.contains("Network:")));
    assert!(result.iter().any(|l| l.contains("localhost:3000")));
    assert!(result.iter().any(|l| l.contains("Ready")));
}

#[test]
fn drops_compiling_lines_in_dev_mode() {
    let lines = vec![
        " ✓ Ready in 2.3s".to_string(),
        "○ Compiling /api/users ...".to_string(),
        "○ Compiling /dashboard ...".to_string(),
        " ✓ Compiled /api/users in 234ms (120 modules)".to_string(),
        "GET /api/users 200 in 241ms".to_string(),
    ];
    let result = NextjsHandler.compress("next dev", lines, &Config::default());
    assert!(!result.iter().any(|l| l.starts_with("○ Compiling")));
    assert!(result.iter().any(|l| l.contains("GET /api/users")));
}

#[test]
fn build_mode_keeps_tail() {
    let mut lines: Vec<String> = (0..60)
        .map(|i| format!("verbose build step {}", i))
        .collect();
    lines.push("✓ Compiled successfully".to_string());
    lines.push("Route (app)                              Size".to_string());
    let result = NextjsHandler.compress("next build", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("Compiled successfully")));
}
