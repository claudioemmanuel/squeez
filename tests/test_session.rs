use std::path::PathBuf;

fn tmp_dir(label: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("squeez_sess_{}_{}", label,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn test_new_session_filename_format() {
    let name = squeez::session::new_session_filename();
    // YYYY-MM-DD-HH.jsonl
    assert!(name.ends_with(".jsonl"), "got: {}", name);
    let stem = name.trim_end_matches(".jsonl");
    let parts: Vec<&str> = stem.split('-').collect();
    assert_eq!(parts.len(), 4, "got: {}", name);
    assert_eq!(parts[0].len(), 4); // year
    assert_eq!(parts[1].len(), 2); // month
    assert_eq!(parts[2].len(), 2); // day
    assert_eq!(parts[3].len(), 2); // hour
}

#[test]
fn test_unix_to_date() {
    // 2026-03-23 00:00:00 UTC = 1774224000
    let date = squeez::session::unix_to_date(1_774_224_000);
    assert_eq!(date, "2026-03-23");
}

#[test]
fn test_current_session_roundtrip() {
    let dir = tmp_dir("roundtrip");
    let s = squeez::session::CurrentSession {
        session_file: "2026-03-23-14.jsonl".to_string(),
        total_tokens: 42_000,
        compact_warned: true,
        start_ts: 1_774_656_000,
    };
    s.save(&dir);
    let loaded = squeez::session::CurrentSession::load(&dir).unwrap();
    assert_eq!(loaded.session_file, "2026-03-23-14.jsonl");
    assert_eq!(loaded.total_tokens, 42_000);
    assert!(loaded.compact_warned);
    assert_eq!(loaded.start_ts, 1_774_656_000);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_current_session_missing_returns_none() {
    let dir = tmp_dir("missing");
    assert!(squeez::session::CurrentSession::load(&dir).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_append_event_creates_and_appends() {
    let dir = tmp_dir("append");
    squeez::session::append_event(&dir, "2026-03-23-14.jsonl", r#"{"type":"tool","ts":1}"#);
    squeez::session::append_event(&dir, "2026-03-23-14.jsonl", r#"{"type":"bash","ts":2}"#);
    let content = std::fs::read_to_string(dir.join("2026-03-23-14.jsonl")).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("tool"));
    assert!(lines[1].contains("bash"));
    let _ = std::fs::remove_dir_all(&dir);
}
