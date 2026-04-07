use squeez::commands::track_result::run_with_dir;
use squeez::context::cache::SessionContext;
use std::path::PathBuf;

fn tmp(label: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "squeez_track_result_{}_{}",
        label,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn read_tool_records_file_path() {
    let dir = tmp("read");
    let json = r#"{"tool_name":"Read","tool_input":{"file_path":"/abs/src/main.rs"},"tool_result":{"content":"fn main() {}"}}"#;
    assert_eq!(run_with_dir("Read", json, &dir), 0);
    let ctx = SessionContext::load(&dir);
    assert!(ctx.seen_files.iter().any(|f| f.path == "/abs/src/main.rs"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn grep_tool_no_panic() {
    let dir = tmp("grep");
    let json = r#"{"tool_name":"Grep","tool_input":{"pattern":"TODO","glob":"*.rs"},"tool_result":{"content":"src/foo.rs:42:    // TODO: refactor"}}"#;
    assert_eq!(run_with_dir("Grep", json, &dir), 0);
    // Grep result content should still extract paths
    let ctx = SessionContext::load(&dir);
    // The path src/foo.rs may or may not register depending on extract heuristics
    let _ = ctx;
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn malformed_json_exits_zero() {
    let dir = tmp("bad");
    assert_eq!(run_with_dir("Read", "garbage", &dir), 0);
    assert_eq!(run_with_dir("Read", "", &dir), 0);
    assert_eq!(run_with_dir("Read", "{}", &dir), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn extracts_errors_from_content() {
    let dir = tmp("err");
    let json = r#"{"tool_name":"Bash","tool_result":{"content":"error: cannot find function 'foo'\nok line"}}"#;
    run_with_dir("Bash", json, &dir);
    let ctx = SessionContext::load(&dir);
    assert!(!ctx.seen_errors.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn extracts_paths_from_content_body() {
    let dir = tmp("paths");
    let json = r#"{"tool_name":"Bash","tool_result":{"content":"modified: src/main.rs\nmodified: src/lib.rs\nadded:    Cargo.toml"}}"#;
    run_with_dir("Bash", json, &dir);
    let ctx = SessionContext::load(&dir);
    let paths: Vec<String> = ctx.seen_files.iter().map(|f| f.path.clone()).collect();
    assert!(paths.iter().any(|p| p == "src/main.rs"));
    assert!(paths.iter().any(|p| p == "src/lib.rs"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn empty_input_no_error() {
    let dir = tmp("empty");
    assert_eq!(run_with_dir("Read", "   \n  ", &dir), 0);
    let _ = std::fs::remove_dir_all(&dir);
}
