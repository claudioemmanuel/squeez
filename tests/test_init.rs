use std::path::PathBuf;

fn tmp_dirs(label: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!(
        "squeez_init_{}_{}",
        label,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    let sessions = base.join("sessions");
    let memory = base.join("memory");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::create_dir_all(&memory).unwrap();
    (sessions, memory)
}

#[test]
fn test_init_creates_current_json() {
    let (sessions, memory) = tmp_dirs("creates");
    let cfg = squeez::config::Config::default();
    squeez::commands::init::run_with_dirs(&sessions, &memory, &cfg);
    assert!(sessions.join("current.json").exists());
    let s = squeez::session::CurrentSession::load(&sessions).unwrap();
    assert!(!s.session_file.is_empty());
    assert_eq!(s.total_tokens, 0);
    assert!(!s.compact_warned);
    let _ = std::fs::remove_dir_all(sessions.parent().unwrap());
}

#[test]
fn test_init_returns_zero() {
    let (sessions, memory) = tmp_dirs("zero");
    let cfg = squeez::config::Config::default();
    let code = squeez::commands::init::run_with_dirs(&sessions, &memory, &cfg);
    assert_eq!(code, 0);
    let _ = std::fs::remove_dir_all(sessions.parent().unwrap());
}

#[test]
fn test_init_finalizes_prior_session_to_memory() {
    let (sessions, memory) = tmp_dirs("finalize");
    let prior_file = "2026-03-22-10.jsonl";
    // 1774137600 = 2026-03-22 00:00:00 UTC
    let prior = squeez::session::CurrentSession {
        session_file: prior_file.to_string(),
        total_tokens: 5_000,
        tokens_saved: 0,
        total_calls: 0,
        compact_warned: false,
        state_warned: false,
        start_ts: 1_774_137_600,
    };
    prior.save(&sessions);
    // Write a bash event in the session log
    squeez::session::append_event(
        &sessions,
        prior_file,
        r#"{"type":"bash","in_tk":200,"out_tk":20,"files":["src/foo.rs"],"errors":[],"git":[],"test_summary":"","ts":1774137601}"#,
    );

    let cfg = squeez::config::Config::default();
    squeez::commands::init::run_with_dirs(&sessions, &memory, &cfg);

    let summaries = squeez::memory::read_last_n(&memory, 10);
    assert!(
        !summaries.is_empty(),
        "Expected prior session to be summarised"
    );
    assert_eq!(summaries[0].date, "2026-03-22");
    let _ = std::fs::remove_dir_all(sessions.parent().unwrap());
}

#[test]
fn test_init_no_prior_session_no_crash() {
    let (sessions, memory) = tmp_dirs("noprior");
    let cfg = squeez::config::Config::default();
    let code = squeez::commands::init::run_with_dirs(&sessions, &memory, &cfg);
    assert_eq!(code, 0);
    let _ = std::fs::remove_dir_all(sessions.parent().unwrap());
}

#[test]
fn test_init_double_init_replaces_session() {
    let (sessions, memory) = tmp_dirs("double");
    let cfg = squeez::config::Config::default();
    squeez::commands::init::run_with_dirs(&sessions, &memory, &cfg);
    let first = squeez::session::CurrentSession::load(&sessions).unwrap();

    squeez::commands::init::run_with_dirs(&sessions, &memory, &cfg);
    let second = squeez::session::CurrentSession::load(&sessions).unwrap();

    // After second init: fresh session (0 tokens, not compact_warned)
    assert_eq!(second.total_tokens, 0);
    assert!(!second.compact_warned);
    assert!(second.start_ts >= first.start_ts);
    let _ = std::fs::remove_dir_all(sessions.parent().unwrap());
}

#[test]
fn test_init_empty_session_log_no_panic() {
    // Prior session with current.json but empty JSONL — must not panic
    let (sessions, memory) = tmp_dirs("emptylog");
    let prior_file = "2026-03-23-08.jsonl";
    let prior = squeez::session::CurrentSession {
        session_file: prior_file.to_string(),
        total_tokens: 0,
        tokens_saved: 0,
        total_calls: 0,
        compact_warned: false,
        state_warned: false,
        start_ts: 1_774_224_000,
    };
    prior.save(&sessions);
    std::fs::write(sessions.join(prior_file), b"").unwrap();

    let cfg = squeez::config::Config::default();
    let code = squeez::commands::init::run_with_dirs(&sessions, &memory, &cfg);
    assert_eq!(code, 0, "empty session log must not crash");
    let _ = std::fs::remove_dir_all(sessions.parent().unwrap());
}
