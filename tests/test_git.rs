use squeez::commands::{git::GitHandler, Handler};
use squeez::config::Config;

fn config() -> Config {
    Config::default()
}

#[test]
fn groups_many_modified_files() {
    let mut lines = vec!["On branch main".to_string()];
    for i in 0..25 {
        lines.push(format!("\tmodified:   src/components/C{}.tsx", i));
    }
    let result = GitHandler.compress("git status", lines, &config());
    assert!(result.iter().any(|l| l.contains("25 modified")));
    assert!(result.iter().any(|l| l.contains("On branch")));
}

#[test]
fn git_log_truncates_to_config_limit() {
    let lines: Vec<String> = (0..100)
        .map(|i| format!("abc{:04} subject {}", i, i))
        .collect();
    let result = GitHandler.compress("git log --oneline", lines, &config());
    assert!(result.len() <= 21); // 20 commits + notice
}

#[test]
fn git_diff_uses_diff_limit() {
    let lines: Vec<String> = (0..300).map(|i| format!("+line {}", i)).collect();
    let result = GitHandler.compress("git diff", lines, &config());
    assert!(result.len() <= 152);
}
