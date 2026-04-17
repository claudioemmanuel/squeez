use squeez::commands::{cloudflare::CloudflareHandler, Handler};
use squeez::config::Config;

#[test]
fn drops_banner_and_separators() {
    let lines = vec![
        "⛅️ wrangler 3.78.0".to_string(),
        "──────────────────────────────────────────".to_string(),
        "Retrieving cached values for the dev session...".to_string(),
        "[mf:inf] Ready on http://127.0.0.1:8787".to_string(),
    ];
    let result = CloudflareHandler.compress("wrangler dev", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("Ready")));
    assert!(!result.iter().any(|l| l.contains("wrangler 3")));
    assert!(!result.iter().any(|l| l.contains("Retrieving cached")));
}

#[test]
fn drops_binding_details() {
    let lines = vec![
        "Your worker has access to the following bindings:".to_string(),
        "- KV: MY_KV (abc123)".to_string(),
        "- D1: MY_DB (def456)".to_string(),
        "Total Upload: 68 KiB / gzip: 18.9 KiB".to_string(),
        "Deployed my-worker triggers (1.23 sec)".to_string(),
        "  https://my-worker.example.workers.dev".to_string(),
    ];
    let result = CloudflareHandler.compress("wrangler deploy", lines, &Config::default());
    assert!(!result.iter().any(|l| l.contains("Your worker has access")));
    assert!(!result.iter().any(|l| l.contains("- KV:")));
    assert!(result.iter().any(|l| l.contains("Deployed")));
    assert!(result.iter().any(|l| l.contains("workers.dev")));
}

#[test]
fn drops_update_notice() {
    let lines = vec![
        "⛅️ wrangler 3.78.0 (update available 3.79.0)".to_string(),
        "npm install wrangler@latest".to_string(),
        "[mf:inf] Ready on http://127.0.0.1:8787".to_string(),
    ];
    let result = CloudflareHandler.compress("wrangler dev", lines, &Config::default());
    assert!(!result.iter().any(|l| l.contains("update available")));
    assert!(!result.iter().any(|l| l.contains("npm install wrangler")));
    assert!(result.iter().any(|l| l.contains("Ready")));
}
