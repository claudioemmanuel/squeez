use squeez::commands::{database::DatabaseHandler, Handler};
use squeez::config::Config;

#[test]
fn prisma_generate_keeps_only_result_line() {
    let lines = vec![
        "Prisma schema loaded from prisma/schema.prisma".to_string(),
        "Environment variables loaded from .env".to_string(),
        "Prisma schema loaded from prisma/schema.prisma".to_string(),
        "✔ Generated Prisma Client (v5.14.0) to ./node_modules/@prisma/client in 234ms".to_string(),
        "".to_string(),
        "Run Prisma Migrate to update your database schema: https://pris.ly/d/migrate".to_string(),
    ];
    let result = DatabaseHandler.compress("npx prisma generate", lines, &Config::default());
    assert_eq!(result.len(), 1);
    assert!(result[0].contains("Generated Prisma Client"));
}

#[test]
fn prisma_generate_passes_error_lines_through() {
    let lines = vec![
        "Prisma schema loaded from prisma/schema.prisma".to_string(),
        "error: Schema parsing error: Unknown field type `Foobar`".to_string(),
    ];
    let result = DatabaseHandler.compress("prisma generate", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("error")));
}

#[test]
fn prisma_migrate_unaffected() {
    let lines = vec![
        "+----+----------+".to_string(),
        "| id | name     |".to_string(),
        "+----+----------+".to_string(),
        "| 1  | Alice    |".to_string(),
        "+----+----------+".to_string(),
    ];
    let result = DatabaseHandler.compress("prisma migrate status", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("Alice")));
    assert!(!result.iter().any(|l| l.starts_with('+')));
}

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
