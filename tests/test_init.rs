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
    // Use a timestamp within the retention window (default 30 days) so the
    // just-written summary is not pruned by init's prune_old call. Hardcoding
    // absolute dates makes this test time-bomb as the clock advances.
    let now = squeez::session::unix_now();
    let start_ts = now.saturating_sub(3600); // 1 hour ago
    let expected_date = squeez::session::unix_to_date(start_ts);
    let prior_file = format!("{}-0.jsonl", expected_date);
    let prior = squeez::session::CurrentSession {
        session_file: prior_file.clone(),
        total_tokens: 5_000,
        tokens_saved: 0,
        total_calls: 0,
        compact_warned: false,
        state_warned: false,
        start_ts,
    };
    prior.save(&sessions);
    squeez::session::append_event(
        &sessions,
        &prior_file,
        &format!(
            r#"{{"type":"bash","in_tk":200,"out_tk":20,"files":["src/foo.rs"],"errors":[],"git":[],"test_summary":"","ts":{}}}"#,
            start_ts + 1
        ),
    );

    let cfg = squeez::config::Config::default();
    squeez::commands::init::run_with_dirs(&sessions, &memory, &cfg);

    let summaries = squeez::memory::read_last_n(&memory, 10);
    assert!(
        !summaries.is_empty(),
        "Expected prior session to be summarised"
    );
    assert_eq!(summaries[0].date, expected_date);
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
