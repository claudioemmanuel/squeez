use squeez::commands::{Handler, build::BuildHandler};
use squeez::config::Config;

#[test]
fn gradle_drops_task_progress_keeps_errors() {
    let lines = vec![
        "> Task :compileJava".to_string(),
        "> Task :processResources".to_string(),
        "error: cannot find symbol".to_string(),
        "  symbol: class Foo".to_string(),
        "BUILD FAILED in 3s".to_string(),
    ];
    let result = BuildHandler.compress("gradle build", lines, &Config::default());
    assert!(!result.iter().any(|l| l.starts_with("> Task")));
    assert!(result.iter().any(|l| l.contains("BUILD FAILED")));
    assert!(result.iter().any(|l| l.contains("error")));
}
