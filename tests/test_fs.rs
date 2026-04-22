use squeez::commands::{fs::FsHandler, Handler};
use squeez::config::Config;

#[test]
fn find_truncates_to_config_limit() {
    let lines: Vec<String> = (0..200).map(|i| format!("./src/file_{}.ts", i)).collect();
    let result = FsHandler.compress("find . -name '*.ts'", lines, &Config::default());
    assert!(result.len() <= 52);
}

#[test]
fn env_strips_high_noise_vars() {
    let lines = vec![
        "PATH=/usr/bin:/usr/local/bin:/very/long/path".to_string(),
        "LS_COLORS=rs=0:di=01;34:ln=01;36:...very long...".to_string(),
        "TERM=xterm-256color".to_string(),
        "NODE_ENV=production".to_string(),
    ];
    let result = FsHandler.compress("env", lines, &Config::default());
    assert!(!result.iter().any(|l| l.starts_with("LS_COLORS")));
    assert!(result.iter().any(|l| l.contains("NODE_ENV")));
}

#[test]
fn tail_command_keeps_tail() {
    // 200 lines, find_max_results default 50 → truncation notice + 50 tail lines.
    // With Keep::Tail we expect the LAST line (line_199) to survive and the
    // FIRST line (line_0) to be dropped.
    let lines: Vec<String> = (0..200).map(|i| format!("line_{}", i)).collect();
    let result = FsHandler.compress("tail /var/log/app.log", lines, &Config::default());
    assert!(result.iter().any(|l| l == "line_199"), "tail line missing");
    assert!(!result.iter().any(|l| l == "line_0"), "head line should have been truncated");
}

#[test]
fn cat_log_file_keeps_tail() {
    let lines: Vec<String> = (0..200).map(|i| format!("event_{}", i)).collect();
    let result = FsHandler.compress("cat /tmp/build.log", lines, &Config::default());
    assert!(result.iter().any(|l| l == "event_199"), "recent log line missing");
    assert!(!result.iter().any(|l| l == "event_0"), "old log line should have been truncated");
}

#[test]
fn cat_non_log_keeps_head() {
    // `cat README.md` should still prefer head (default behavior unchanged).
    let lines: Vec<String> = (0..200).map(|i| format!("line_{}", i)).collect();
    let result = FsHandler.compress("cat README.md", lines, &Config::default());
    assert!(result.iter().any(|l| l == "line_0"), "head line missing for non-log cat");
}
