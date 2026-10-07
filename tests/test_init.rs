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
        overhead_tokens: 0,
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
        overhead_tokens: 0,
    };
    prior.save(&sessions);
    std::fs::write(sessions.join(prior_file), b"").unwrap();

    let cfg = squeez::config::Config::default();
    let code = squeez::commands::init::run_with_dirs(&sessions, &memory, &cfg);
    assert_eq!(code, 0, "empty session log must not crash");
    let _ = std::fs::remove_dir_all(sessions.parent().unwrap());
}

fn prior_session_context(sessions: &std::path::Path) -> squeez::context::cache::SessionContext {
    let mut ctx = squeez::context::cache::SessionContext::default();
    // Same hour as the session about to start: the hourly name cannot be what
    // tells the two apart.
    ctx.session_file = squeez::session::new_session_filename();
    ctx.call_counter = 500;
    ctx.note_agent_spawn("Agent", 350_000);
    ctx.note_agent_measured(4_000_000);
    ctx.note_tool_tokens("Bash", 900_000);
    ctx.note_burn(900_000);
    ctx.note_file("src/kept.rs", squeez::context::cache::FileAccess::Read);
    ctx.save(sessions);
    ctx
}

#[test]
fn session_start_resets_the_previous_sessions_counters() {
    let (sessions, memory) = tmp_dirs("counters");
    let cfg = squeez::config::Config::default();
    prior_session_context(&sessions);

    squeez::commands::init::run_with_dirs(&sessions, &memory, &cfg);

    let ctx = squeez::context::cache::SessionContext::load(&sessions);
    assert_eq!(ctx.agent_spawns, 0);
    assert_eq!(ctx.agent_measured_tokens, 0);
    assert_eq!(ctx.agent_measured_count, 0);
    assert_eq!(ctx.agent_estimated_tokens, 0);
    assert!(ctx.agent_spawn_log.is_empty());
    assert_eq!(ctx.tokens_bash, 0);
    assert!(ctx.burn_window.is_empty());
    assert!(
        squeez::economy::agent_tracker::agent_cost_warning(&ctx, &cfg).is_none(),
        "a session that spawned nothing must not report the last one's agents"
    );
    assert!(!ctx.seen_files.is_empty(), "file history survives the session change");
    let _ = std::fs::remove_dir_all(sessions.parent().unwrap());
}

#[test]
fn copy_loaded_before_session_start_cannot_undo_it() {
    let (sessions, memory) = tmp_dirs("stale");
    let cfg = squeez::config::Config::default();
    prior_session_context(&sessions);

    // A wrapped command loads the context, then runs for a while.
    let mut stale = squeez::context::cache::SessionContext::load(&sessions);
    // Meanwhile a new session starts.
    squeez::commands::init::run_with_dirs(&sessions, &memory, &cfg);
    let started = squeez::context::cache::SessionContext::load(&sessions);
    // The command finishes and writes back what it loaded.
    stale.next_call_n();
    assert!(!stale.save_unless_superseded(&sessions), "stale copy is dropped");

    let ctx = squeez::context::cache::SessionContext::load(&sessions);
    assert_eq!(ctx.session_epoch, started.session_epoch, "session stamp kept");
    assert_eq!(ctx.dedup_floor_call, started.dedup_floor_call, "dedup floor kept");
    assert_eq!(ctx.agent_spawns, 0, "counters stay reset");
    let _ = std::fs::remove_dir_all(sessions.parent().unwrap());
}

#[test]
fn copy_loaded_in_the_current_session_is_saved() {
    let (sessions, _memory) = tmp_dirs("same");
    prior_session_context(&sessions);

    let mut ctx = squeez::context::cache::SessionContext::load(&sessions);
    ctx.next_call_n();
    assert!(ctx.save_unless_superseded(&sessions));
    assert_eq!(
        squeez::context::cache::SessionContext::load(&sessions).call_counter,
        ctx.call_counter
    );
    let _ = std::fs::remove_dir_all(sessions.parent().unwrap());
}
