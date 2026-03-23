use squeez::commands::{runtime::RuntimeHandler, Handler};
use squeez::config::Config;

#[test]
fn node_stacktrace_drops_node_modules_frames() {
    let lines = vec![
        "Error: ENOENT: no such file or directory".to_string(),
        "    at Object.<anonymous> (/app/src/index.js:10:5)".to_string(),
        "    at Module._compile (/app/node_modules/webpack/lib/index.js:50:10)".to_string(),
        "    at Object.Module._extensions (/app/node_modules/other/lib.js:1:1)".to_string(),
        "    at internal/modules/cjs/loader.js:1137:14".to_string(),
    ];
    let result = RuntimeHandler.compress("node app.js", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("ENOENT")));
    assert!(result.iter().any(|l| l.contains("src/index.js")));
    assert!(!result.iter().any(|l| l.contains("node_modules/webpack")));
    assert!(!result.iter().any(|l| l.contains("node_modules/other")));
}
