use squeez::context::cache::{raw_read_hint, SessionContext};
use std::path::PathBuf;

fn tmp_dir(label: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "squeez_ctxcache_{}_{}",
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
fn save_load_round_trip() {
    let dir = tmp_dir("rt");
    let mut c = SessionContext::default();
    c.session_file = "2026-04-07-12.jsonl".to_string();
    c.next_call_n();
    c.record_call("git status", 0xcafe_babe, 200, 1);
    c.note_files(&["src/main.rs".to_string(), "Cargo.toml".to_string()]);
    c.note_errors(&["error: cannot find function 'foo'".to_string()]);
    c.note_git(&["abc1234 commit msg".to_string()]);
    c.save(&dir);

    let loaded = SessionContext::load(&dir);
    assert_eq!(loaded.session_file, "2026-04-07-12.jsonl");
    assert_eq!(loaded.call_counter, 1);
    assert_eq!(loaded.call_log.len(), 1);
    assert_eq!(loaded.call_log[0].output_hash, 0xcafe_babe);
    assert_eq!(loaded.call_log[0].output_len, 200);
    assert_eq!(loaded.seen_files.len(), 2);
    assert_eq!(loaded.seen_errors.len(), 1);
    assert_eq!(loaded.seen_git_refs, vec!["abc1234"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_missing_returns_empty() {
    let dir = tmp_dir("miss");
    let c = SessionContext::load(&dir);
    assert_eq!(c.call_counter, 0);
    assert!(c.call_log.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ring_buffer_drops_oldest_at_overflow() {
    let mut c = SessionContext::default();
    for i in 0..40 {
        let n = c.next_call_n();
        c.record_call(&format!("c{}", i), i, i as usize, n);
    }
    assert_eq!(c.call_log.len(), 32);
    assert!(c.call_log[0].call_n > 1);
}

#[test]
fn note_files_caps_at_max() {
    let mut c = SessionContext::default();
    c.next_call_n();
    for i in 0..400 {
        c.note_files(&[format!("/p/{}.rs", i)]);
    }
    assert!(c.seen_files.len() <= 256);
}

#[test]
fn raw_read_hint_for_seen_file() {
    let mut c = SessionContext::default();
    c.next_call_n();
    c.note_files(&["src/main.rs".to_string()]);
    let h = raw_read_hint(&c, "cat src/main.rs");
    assert!(h.is_some());
    let s = h.unwrap();
    assert!(s.contains("src/main.rs"));
    assert!(s.contains("squeez hint"));
}

#[test]
fn raw_read_hint_skips_non_read_commands() {
    let mut c = SessionContext::default();
    c.next_call_n();
    c.note_files(&["src/main.rs".to_string()]);
    assert!(raw_read_hint(&c, "git status").is_none());
    assert!(raw_read_hint(&c, "ls -la").is_none());
}

#[test]
fn raw_read_hint_skips_unseen_file() {
    let mut c = SessionContext::default();
    c.next_call_n();
    c.note_files(&["src/main.rs".to_string()]);
    assert!(raw_read_hint(&c, "cat src/lib.rs").is_none());
}
