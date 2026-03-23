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
