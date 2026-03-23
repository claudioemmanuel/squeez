use squeez::commands::generic::GenericHandler;
use squeez::commands::Handler;
use squeez::config::Config;

#[test]
fn truncates_to_max_lines() {
    let lines: Vec<String> = (0..300).map(|i| format!("line {}", i)).collect();
    let config = Config::default();
    let result = GenericHandler.compress("somecommand", lines, &config);
    assert!(result.len() <= 201);
}

#[test]
fn deduplicates() {
    let lines: Vec<String> = std::iter::repeat("same".to_string()).take(10).collect();
    let config = Config::default();
    let result = GenericHandler.compress("somecommand", lines, &config);
    assert_eq!(result.len(), 1);
    assert!(result[0].contains("[×10]"));
}
