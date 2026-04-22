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
    // Uses a non-.md extension (`.txt`) so the md-mode branch doesn't fire.
    let lines: Vec<String> = (0..200).map(|i| format!("line_{}", i)).collect();
    let result = FsHandler.compress("cat NOTES.txt", lines, &Config::default());
    assert!(result.iter().any(|l| l == "line_0"), "head line missing for non-log cat");
}

fn make_md_lines(n: usize) -> Vec<String> {
    let mut lines = Vec::with_capacity(n);
    lines.push("# Project Plan".to_string());
    lines.push(String::new());
    for i in 0..n {
        match i % 10 {
            0 => lines.push(format!("## Section {}", i / 10 + 1)),
            1 => lines.push(String::new()),
            _ => lines.push(format!(
                "This is just really basically a simple paragraph {} that you should basically read carefully.",
                i
            )),
        }
    }
    lines
}

#[test]
fn cat_markdown_routes_through_compress_md() {
    let mut cfg = Config::default();
    cfg.auto_compress_md = true;
    cfg.sig_mode_enabled = false;

    let lines = make_md_lines(220);
    let input_len: usize = lines.iter().map(|l| l.len() + 1).sum();

    let result = FsHandler.compress("cat plan.md", lines, &cfg);

    // marker present
    assert!(
        result[0].contains("[squeez: md-mode"),
        "expected md-mode marker, got: {}",
        result[0]
    );
    // output shorter than input
    let output_len: usize = result.iter().map(|l| l.len() + 1).sum();
    assert!(
        output_len < input_len,
        "expected output shorter than input: {} vs {}",
        output_len,
        input_len
    );
}

#[test]
fn cat_markdown_respects_auto_compress_md_disabled() {
    let mut cfg = Config::default();
    cfg.auto_compress_md = false;
    cfg.sig_mode_enabled = false;

    let lines = make_md_lines(220);
    let result = FsHandler.compress("cat plan.md", lines, &cfg);

    assert!(
        !result[0].contains("[squeez: md-mode"),
        "md-mode marker should not appear when auto_compress_md=false"
    );
}

#[test]
fn cat_non_markdown_unchanged_path() {
    let mut cfg = Config::default();
    cfg.auto_compress_md = true;
    cfg.sig_mode_enabled = false;

    let lines: Vec<String> = (0..30).map(|i| format!("line {}", i)).collect();
    let result = FsHandler.compress("cat output.log", lines, &cfg);

    assert!(
        !result.iter().any(|l| l.contains("[squeez: md-mode")),
        "md-mode marker must not appear for non-.md files"
    );
}
