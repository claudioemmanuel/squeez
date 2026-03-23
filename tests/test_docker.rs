use squeez::commands::{docker::DockerHandler, Handler};
use squeez::config::Config;

#[test]
fn docker_logs_keeps_tail() {
    let lines: Vec<String> = (0..200)
        .map(|i| format!("[2026-03-22T00:00:{:02}Z] msg {}", i % 60, i))
        .collect();
    let result = DockerHandler.compress("docker logs foo", lines, &Config::default());
    assert!(result.len() <= 102);
    assert!(!result
        .iter()
        .any(|l| l.starts_with('[') && l.contains('T') && l.contains('Z')));
}

#[test]
fn docker_deduplicates() {
    let lines: Vec<String> = std::iter::repeat("WARN pool exhausted".to_string())
        .take(20)
        .collect();
    let result = DockerHandler.compress("docker logs foo", lines, &Config::default());
    assert_eq!(result.len(), 1);
    assert!(result[0].contains("[×20]"));
}
