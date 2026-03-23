use std::path::PathBuf;

fn tmp_dir(label: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "squeez_track_{}_{}",
        label,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn seed_session(dir: &PathBuf, filename: &str) {
    let s = squeez::session::CurrentSession {
        session_file: filename.to_string(),
        total_tokens: 0,
        compact_warned: false,
        start_ts: 1_000,
    };
    s.save(dir);
}

#[test]
fn test_track_accumulates_tokens() {
    let dir = tmp_dir("accum");
    seed_session(&dir, "2026-03-23-14.jsonl");

    squeez::commands::track::run_with_dir("Read", "4000", &dir); // 4000/4 = 1000 tk
    squeez::commands::track::run_with_dir("Bash", "8000", &dir); // 8000/4 = 2000 tk

    let s = squeez::session::CurrentSession::load(&dir).unwrap();
    assert_eq!(s.total_tokens, 3_000);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_track_appends_event() {
    let dir = tmp_dir("event");
    seed_session(&dir, "2026-03-23-14.jsonl");
    squeez::commands::track::run_with_dir("Grep", "400", &dir);
    let log = std::fs::read_to_string(dir.join("2026-03-23-14.jsonl")).unwrap();
    assert!(log.contains("Grep"), "got: {}", log);
    assert!(log.contains("tokens_est"), "got: {}", log);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_track_no_session_is_noop() {
    let dir = tmp_dir("noop");
    // No current.json — should succeed silently
    let code = squeez::commands::track::run_with_dir("Read", "1000", &dir);
    assert_eq!(code, 0);
    assert!(!dir.join("current.json").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
