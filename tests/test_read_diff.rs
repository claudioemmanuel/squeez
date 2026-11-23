//! Diff on re-read, end to end through the PostToolUse rewrite. One test:
//! it points SQUEEZ_DIR (process-global) at a temp dir for the stash.
use squeez::commands::compress_output::compute_rewrite;
use squeez::config::Config;
use squeez::context::cache::SessionContext;

fn read_payload(path: &str, body: &str, extra_input: &str) -> String {
    let n = body.lines().count();
    let esc = body.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
    format!(
        r#"{{"tool_name":"Read","tool_input":{{"file_path":"{path}"{extra_input}}},"tool_response":{{"type":"text","file":{{"filePath":"{path}","content":"{esc}","numLines":{n},"startLine":1,"totalLines":{n}}}}}}}"#
    )
}

#[test]
fn reread_after_small_edit_is_a_hunk() {
    let root = std::env::temp_dir().join(format!("squeez-readdiff-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("SQUEEZ_DIR", &root);
    let sessions = root.join("sessions");
    let cfg = Config::default();
    let v1: Vec<String> = (1..=200).map(|i| format!("fn item_{i}() -> u32 {{ {i} }}")).collect();
    let mut v2 = v1.clone();
    v2[99] = "fn item_100() -> u32 { 1000 }".into();

    assert_eq!(compute_rewrite(&read_payload("/p/a.rs", &v1.join("\n"), ""), "Read", &sessions, &cfg), None);
    let note = compute_rewrite(&read_payload("/p/a.rs", &v2.join("\n"), ""), "Read", &sessions, &cfg)
        .expect("hunk instead of the whole file");
    assert!(note.contains("unchanged since Read #"), "{note}");
    assert!(note.contains("-fn item_100() -> u32 { 100 }\n+fn item_100() -> u32 { 1000 }"), "{note}");
    assert!(note.len() < v2.join("\n").len() / 5);

    // A partial read is never diffed.
    let mut v3 = v2.clone();
    v3[10] = "changed".into();
    let partial = compute_rewrite(&read_payload("/p/a.rs", &v3.join("\n"), r#","offset":1"#), "Read", &sessions, &cfg);
    assert!(!partial.unwrap_or_default().contains("except the hunk"));

    // A new session: the earlier read is no longer in the model's context.
    SessionContext::update(&sessions, |c| c.begin_session("next"));
    let mut v4 = v2.clone();
    v4[5] = "changed again".into();
    let fresh = compute_rewrite(&read_payload("/p/a.rs", &v4.join("\n"), ""), "Read", &sessions, &cfg);
    assert!(!fresh.unwrap_or_default().contains("except the hunk"));
    let _ = std::fs::remove_dir_all(&root);
}
