use squeez::commands::compress_output;
use squeez::config::Config;

fn tmp() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CTR: AtomicU64 = AtomicU64::new(0);
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "squeez_agent_co_{}_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        n
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn agent_small_output_not_rewritten() {
    let dir = tmp();
    let cfg = Config::default();
    let json = r#"{"tool_name":"Agent","tool_result":{"content":"Done. Project explored."}}"#;
    assert_eq!(compress_output::run_with(json, "Agent", &dir, &cfg), 0);
    // Small output: no rewrite should occur (exits cleanly)
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn agent_large_output_summarized() {
    let dir = tmp();
    let mut cfg = Config::default();
    cfg.redundancy_cache_enabled = false;

    // 600+ lines — above summarize_threshold_lines (default 300)
    let mut content = String::from("## Full Directory Structure\\n\\n");
    for i in 0..120 {
        content.push_str(&format!(
            "- `src/module_{}/index.ts` — module {} implementation\\n",
            i, i
        ));
        content.push_str(&format!(
            "- `src/module_{}/types.ts` — module {} types\\n",
            i, i
        ));
        content.push_str(&format!(
            "- `src/module_{}/utils.ts` — module {} utilities\\n",
            i, i
        ));
        content.push_str(&format!(
            "- `src/module_{}/tests.ts` — module {} tests\\n",
            i, i
        ));
        content.push_str(&format!(
            "- `src/module_{}/README.md` — module {} docs\\n\\n",
            i, i
        ));
    }
    let json = format!(
        r#"{{"tool_name":"Agent","tool_result":{{"content":"{}"}}}}"#,
        content
    );
    let rewrite = compress_output::compute_rewrite(&json, "Agent", &dir, &cfg);
    assert!(
        rewrite.is_some(),
        "expected summarize to fire for 600-line agent output"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn agent_identical_output_deduplicated() {
    let dir = tmp();
    let cfg = Config::default();

    let lines: String = (0..60)
        .map(|i| format!("line {}: agent output content for item {}", i, i))
        .collect::<Vec<_>>()
        .join("\\n");
    let json = format!(
        r#"{{"tool_name":"Agent","tool_result":{{"content":"{}"}}}}"#,
        lines
    );

    // First call records the content
    compress_output::run_with(&json, "Agent", &dir, &cfg);
    // Second identical call should be deduplicated
    let rewrite = compress_output::compute_rewrite(&json, "Agent", &dir, &cfg);
    assert!(
        rewrite.is_some(),
        "second identical agent output should be deduplicated"
    );
    let note = rewrite.unwrap();
    assert!(
        note.contains("identical") || note.contains("similar"),
        "expected dedup note, got: {}",
        note
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn task_tool_same_pipeline_as_agent() {
    let dir = tmp();
    let cfg = Config::default();

    let content: String = (0..60)
        .map(|i| format!("task result line {}", i))
        .collect::<Vec<_>>()
        .join("\\n");
    let json = format!(
        r#"{{"tool_name":"Task","tool_result":{{"content":"{}"}}}}"#,
        content
    );

    compress_output::run_with(&json, "Task", &dir, &cfg);
    let rewrite = compress_output::compute_rewrite(&json, "Task", &dir, &cfg);
    assert!(
        rewrite.is_some(),
        "Task tool should deduplicate identical output same as Agent"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
