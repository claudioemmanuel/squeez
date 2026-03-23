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
        compact_warned: false,
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
