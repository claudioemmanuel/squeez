use squeez::commands::{wrangler::WranglerHandler, Handler};
use squeez::config::Config;

#[test]
fn drops_upload_progress_keeps_errors_and_url() {
    let lines = vec![
        "⛅️ wrangler 3.78.0".to_string(),
        "-------------------".to_string(),
        "Total Upload: 512.4 KiB / gzip: 120.3 KiB".to_string(),
        "  src/index.ts   12.1 KiB / gzip: 4.2 KiB".to_string(),
        "  src/worker.ts  8.3 KiB / gzip: 2.1 KiB".to_string(),
        "✘ [ERROR] Binding MY_KV not found".to_string(),
        "Uploaded my-worker (2.34 sec)".to_string(),
        "Deployed my-worker triggers (1.02 sec)".to_string(),
        "  https://my-worker.acme.workers.dev".to_string(),
        "Current Version ID: abc123".to_string(),
    ];
    let result = WranglerHandler.compress("wrangler deploy", lines, &Config::default());
    assert!(!result.iter().any(|l| l.contains("KiB / gzip:")));
    assert!(!result.iter().any(|l| l.starts_with("-----")));
    assert!(result.iter().any(|l| l.contains("[ERROR] Binding MY_KV")));
    assert!(result.iter().any(|l| l.contains("workers.dev")));
}

#[test]
fn d1_execute_keeps_row_summary() {
    let lines = vec![
        "🌀 Executing on remote database my-db (abc)".to_string(),
        "🚣 Executed 1 command in 12.3ms".to_string(),
        "┌────┬──────┐".to_string(),
        "│ id │ name │".to_string(),
        "├────┼──────┤".to_string(),
        "│  1 │ foo  │".to_string(),
        "└────┴──────┘".to_string(),
    ];
    let result = WranglerHandler.compress(
        "wrangler d1 execute my-db --command \"SELECT * FROM t\"",
        lines,
        &Config::default(),
    );
    assert!(result.iter().any(|l| l.contains("Executed 1 command")));
    assert!(result.iter().any(|l| l.contains("foo")));
}
