use squeez::memory::{self, Summary};
use std::io::Write;
use std::path::PathBuf;

fn tmp_dir(label: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "squeez_memfix_{}_{}",
        label,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sample_summary(ts: u64) -> Summary {
    Summary {
        date: "2026-04-19".to_string(),
        duration_min: 10,
        tokens_saved: 1000,
        files_touched: vec!["src/main.rs".to_string()],
        files_committed: vec![],
        test_summary: String::new(),
        errors_resolved: vec![],
        git_events: vec![],
        ts,
        valid_from: ts,
        valid_to: 0,
        investigated: vec![],
        learned: vec![],
        completed: vec![],
        next_steps: vec![],
        compression_ratio_bp: 0,
        tool_choice_efficiency_bp: 0,
        context_reuse_rate_bp: 0,
        budget_utilization_bp: 0,
        efficiency_overall_bp: 0,
    }
}

// ── Fix 2: read_last_n streaming ─────────────────────────────────────────

#[test]
fn test_read_last_n_returns_most_recent() {
    let dir = tmp_dir("read_last_n");
    // Write 100 summaries with increasing ts
    for i in 1..=100u64 {
        memory::write_summary(&dir, &sample_summary(i));
    }
    let result = memory::read_last_n(&dir, 5);
    assert_eq!(result.len(), 5);
    // Most recent first
    assert_eq!(result[0].ts, 100);
    assert_eq!(result[1].ts, 99);
    assert_eq!(result[4].ts, 96);
}

#[test]
fn test_read_last_n_empty_file() {
    let dir = tmp_dir("read_last_n_empty");
    let _ = std::fs::write(dir.join("summaries.jsonl"), "");
    let result = memory::read_last_n(&dir, 5);
    assert!(result.is_empty());
}

#[test]
fn test_read_last_n_fewer_than_n() {
    let dir = tmp_dir("read_last_n_few");
    memory::write_summary(&dir, &sample_summary(10));
    memory::write_summary(&dir, &sample_summary(20));
    let result = memory::read_last_n(&dir, 5);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].ts, 20);
    assert_eq!(result[1].ts, 10);
}

#[test]
fn test_read_last_n_zero() {
    let dir = tmp_dir("read_last_n_zero");
    memory::write_summary(&dir, &sample_summary(10));
    let result = memory::read_last_n(&dir, 0);
    assert!(result.is_empty());
}

#[test]
fn test_read_last_n_missing_file() {
    let dir = tmp_dir("read_last_n_missing");
    let result = memory::read_last_n(&dir, 5);
    assert!(result.is_empty());
}

// ── Fix 3: prune_old streaming write ─────────────────────────────────────

#[test]
fn test_prune_old_removes_old_entries() {
    let dir = tmp_dir("prune_old");
    // Write entries: one very old (ts=100), one recent (ts=now)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    memory::write_summary(&dir, &sample_summary(100)); // very old
    memory::write_summary(&dir, &sample_summary(now));  // recent
    memory::prune_old(&dir, 7); // 7 days retention
    let result = memory::read_last_n(&dir, 100);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].ts, now);
}

#[test]
fn test_prune_old_keeps_all_when_recent() {
    let dir = tmp_dir("prune_keep");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    memory::write_summary(&dir, &sample_summary(now - 100));
    memory::write_summary(&dir, &sample_summary(now));
    memory::prune_old(&dir, 7);
    let result = memory::read_last_n(&dir, 100);
    assert_eq!(result.len(), 2);
}

#[test]
fn test_prune_old_empty_file() {
    let dir = tmp_dir("prune_empty");
    let _ = std::fs::write(dir.join("summaries.jsonl"), "");
    memory::prune_old(&dir, 7); // should not panic
}

// ── Fix 5: session_detail HashSet dedup ──────────────────────────────────

#[test]
fn test_session_detail_no_duplicate_files() {
    let dir = tmp_dir("session_detail");
    // Write a session JSONL with duplicate file entries
    let path = dir.join("2026-04-19_test.jsonl");
    let mut f = std::fs::File::create(&path).unwrap();
    for _ in 0..10 {
        writeln!(f, "{{\"type\":\"bash\",\"cmd\":\"ls\",\"in_tk\":100,\"out_tk\":50,\"files\":[\"src/main.rs\",\"src/lib.rs\"],\"errors\":[],\"git\":[],\"test_summary\":\"\",\"ts\":1000}}").unwrap();
    }
    let detail = memory::session_detail(&dir, "2026-04-19");
    // files_seen should be 2, not 20
    assert!(detail.contains("files_seen: 2"), "expected files_seen: 2, got: {}", detail);
}

// ── Fix 6: extract_file_paths cap ────────────────────────────────────────

#[test]
fn test_extract_file_paths_cap_at_100() {
    use squeez::commands::wrap::extract_file_paths;
    // Generate text with 200 unique file paths
    let text: String = (0..200)
        .map(|i| format!("src/file_{}.rs", i))
        .collect::<Vec<_>>()
        .join(" ");
    let paths = extract_file_paths(&text);
    assert_eq!(paths.len(), 100);
}

// ── Fix 4: redundancy no double-join ─────────────────────────────────────

#[test]
fn test_redundancy_check_record_correctness() {
    use squeez::context::cache::SessionContext;
    use squeez::context::redundancy;

    let mut ctx = SessionContext::default();
    let output: Vec<String> = (0..20).map(|i| format!("line {}", i)).collect();

    // Record and verify call_n increments
    let n1 = redundancy::record(&mut ctx, "cmd1", &output);
    assert_eq!(n1, 1);

    // Same output should hit on check
    let hit = redundancy::check(&ctx, &output);
    assert!(hit.is_some());
    assert_eq!(hit.unwrap().call_n, 1);

    // Different output should not hit
    let output2: Vec<String> = (0..20).map(|i| format!("different {}", i)).collect();
    let hit2 = redundancy::check(&ctx, &output2);
    assert!(hit2.is_none());
}

// ── Fix 1: MCP ctx cache ────────────────────────────────────────────────

#[test]
fn test_mcp_handle_request_basic() {
    // Verify the MCP server still works with the caching changes
    use squeez::commands::mcp_server::handle_request;
    let req = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}";
    let resp = handle_request(req).expect("must respond");
    assert!(resp.contains("\"protocolVersion\""));
}
