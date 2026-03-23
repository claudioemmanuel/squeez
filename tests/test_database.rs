use squeez::commands::{database::DatabaseHandler, Handler};
use squeez::config::Config;

#[test]
fn strips_sql_border_lines() {
    let lines = vec![
        "+----+----------+".to_string(),
        "| id | name     |".to_string(),
        "+----+----------+".to_string(),
        "| 1  | Alice    |".to_string(),
        "| 2  | Bob      |".to_string(),
        "+----+----------+".to_string(),
        "(2 rows)".to_string(),
    ];
    let result = DatabaseHandler.compress("psql", lines, &Config::default());
    assert!(!result.iter().any(|l| l.starts_with('+')));
    assert!(result.iter().any(|l| l.contains("Alice")));
    assert!(result.iter().any(|l| l.contains("2 rows")));
}
