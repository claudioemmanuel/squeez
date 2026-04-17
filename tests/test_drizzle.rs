use squeez::commands::{drizzle::DrizzleHandler, Handler};
use squeez::config::Config;

#[test]
fn drops_boilerplate_keeps_migration_info() {
    let lines = vec![
        "drizzle-kit: v0.21.4".to_string(),
        "database credentials from drizzle.config.ts".to_string(),
        "Reading config file '/home/user/project/drizzle.config.ts'".to_string(),
        "Using 'pg' driver for database querying".to_string(),
        "dialect is 'postgresql'".to_string(),
        "[✓] Your schema has 2 changes".to_string(),
        "  - Added table 'users'".to_string(),
        "  - Added column 'email' to 'accounts'".to_string(),
        "[✓] 0001_init.sql generated".to_string(),
    ];
    let result = DrizzleHandler.compress("drizzle-kit generate", lines, &Config::default());
    assert!(!result.iter().any(|l| l.contains("drizzle-kit: v")));
    assert!(!result.iter().any(|l| l.contains("database credentials from")));
    assert!(!result.iter().any(|l| l.contains("Reading config file")));
    assert!(!result.iter().any(|l| l.contains("Using 'pg' driver")));
    assert!(!result.iter().any(|l| l.contains("dialect is")));
    assert!(result.iter().any(|l| l.contains("0001_init.sql")));
    assert!(result.iter().any(|l| l.contains("users")));
}

#[test]
fn keeps_error_output() {
    let lines = vec![
        "drizzle-kit: v0.21.4".to_string(),
        "Reading config file '/home/user/project/drizzle.config.ts'".to_string(),
        "Error: Cannot connect to database: connection refused".to_string(),
        "  at connect (drizzle-kit/src/index.ts:42:5)".to_string(),
    ];
    let result = DrizzleHandler.compress("drizzle-kit push", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("Error:")));
    assert!(result.iter().any(|l| l.contains("connection refused")));
}

#[test]
fn keeps_no_changes_message() {
    let lines = vec![
        "drizzle-kit: v0.21.4".to_string(),
        "dialect is 'postgresql'".to_string(),
        "[✓] Your schema has no changes, nothing to generate!".to_string(),
    ];
    let result = DrizzleHandler.compress("drizzle-kit generate", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("no changes")));
}
