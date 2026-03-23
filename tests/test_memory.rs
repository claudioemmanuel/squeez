use squeez::memory::Summary;
use std::path::PathBuf;

fn tmp_dir(label: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "squeez_mem_{}_{}",
        label,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sample_summary(date: &str, ts: u64) -> Summary {
    Summary {
        date: date.to_string(),
        duration_min: 30,
        tokens_saved: 50_000,
        files_touched: vec!["src/auth.ts".to_string(), "src/foo.rs".to_string()],
        files_committed: vec!["src/auth.ts".to_string()],
        test_summary: "cargo: 12 passed".to_string(),
        errors_resolved: vec!["error[E0502] in auth.ts".to_string()],
        git_events: vec!["abc1234 fix auth".to_string()],
        ts,
    }
}

#[test]
fn test_summary_jsonl_roundtrip() {
    let s = sample_summary("2026-03-22", 1_774_569_600);
    let line = s.to_jsonl_line();
    let back = Summary::from_jsonl_line(&line).unwrap();
    assert_eq!(back.date, "2026-03-22");
    assert_eq!(back.tokens_saved, 50_000);
    assert_eq!(back.files_touched, vec!["src/auth.ts", "src/foo.rs"]);
    assert_eq!(back.git_events, vec!["abc1234 fix auth"]);
    assert_eq!(back.ts, 1_774_569_600);
}

#[test]
fn test_write_and_read_last_n() {
    let dir = tmp_dir("write_read");
    let s1 = sample_summary("2026-03-21", 1_000);
    let s2 = sample_summary("2026-03-22", 2_000);
    let s3 = sample_summary("2026-03-23", 3_000);
    squeez::memory::write_summary(&dir, &s1);
    squeez::memory::write_summary(&dir, &s2);
    squeez::memory::write_summary(&dir, &s3);
    let results = squeez::memory::read_last_n(&dir, 2);
    assert_eq!(results.len(), 2);
    // Sorted newest first
    assert_eq!(results[0].ts, 3_000);
    assert_eq!(results[1].ts, 2_000);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_read_last_n_empty_dir() {
    let dir = tmp_dir("empty");
    let results = squeez::memory::read_last_n(&dir, 3);
    assert!(results.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_prune_old_removes_stale() {
    let dir = tmp_dir("prune");
    let old_ts = 1_000u64; // very old
    let new_ts = squeez::session::unix_now() - 86400; // 1 day ago
    squeez::memory::write_summary(&dir, &sample_summary("2020-01-01", old_ts));
    squeez::memory::write_summary(&dir, &sample_summary("2026-03-22", new_ts));
    squeez::memory::prune_old(&dir, 30);
    let results = squeez::memory::read_last_n(&dir, 10);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].ts, new_ts);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_display_line_format() {
    let s = sample_summary("2026-03-22", 1_000);
    let line = s.display_line();
    assert!(line.contains("2026-03-22"), "got: {}", line);
    assert!(line.contains("2 file"), "got: {}", line);
    assert!(line.contains("1 commit"), "got: {}", line);
}
