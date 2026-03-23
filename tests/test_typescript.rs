use squeez::commands::{Handler, typescript::TypescriptHandler};
use squeez::config::Config;

#[test]
fn keeps_error_lines_drops_code_context() {
    let lines = vec![
        "src/auth.ts(42,5): error TS2345: Argument of type 'undefined' is not assignable".to_string(),
        "  const x: string = undefined;".to_string(),
        "                    ~~~~~~~~~".to_string(),
        "src/index.ts(10,1): error TS2304: Cannot find name 'foo'".to_string(),
        "Found 2 errors.".to_string(),
    ];
    let result = TypescriptHandler.compress("tsc --noEmit", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("TS2345")));
    assert!(result.iter().any(|l| l.contains("TS2304")));
    assert!(result.iter().any(|l| l.contains("2 errors")));
    assert!(result.len() <= 4);
}
