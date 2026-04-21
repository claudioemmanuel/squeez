/// Tests for Phase 1 structured session summaries (AC-1) and
/// Phase 3 cross-session search functions.
use std::path::PathBuf;

fn tmp_dirs(label: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!(
        "squeez_memst_{}_{}",
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

/// Set up a prior session with the given bash event JSON lines, then call
/// `run_with_dirs` to trigger finalization into the memory dir.
fn finalize_session(sessions: &std::path::Path, memory: &std::path::Path, events: &[&str]) {
    // Use a recent timestamp so prune_old (default 30-day retention)
    // inside run_with_dirs doesn't delete the just-written summary.
    let now = squeez::session::unix_now();
    let start_ts = now.saturating_sub(3600);
    let date = squeez::session::unix_to_date(start_ts);
    let session_file = format!("{}-00.jsonl", date);
    let prior = squeez::session::CurrentSession {
        session_file: session_file.clone(),
        total_tokens: 1000,
        tokens_saved: 0,
        total_calls: 0,
        compact_warned: false,
        state_warned: false,
        start_ts,
    };
    prior.save(sessions);
    for ev in events {
        squeez::session::append_event(sessions, &session_file, ev);
    }
    let cfg = squeez::config::Config::default();
    squeez::commands::init::run_with_dirs(sessions, memory, &cfg);
}

// ── AC-1: structured fields populated ──────────────────────────────────────

#[test]
fn test_investigated_populated_from_files() {
    let (sessions, memory) = tmp_dirs("inv");
    finalize_session(
        &sessions,
        &memory,
        &[r#"{"type":"bash","cmd":"cargo build","in_tk":100,"out_tk":50,"files":["src/main.rs","src/lib.rs"],"errors":[],"git":[],"test_summary":"","ts":1774137601}"#],
    );
    let summaries = squeez::memory::read_last_n(&memory, 1);
    assert!(!summaries.is_empty(), "expected a summary after finalization");
    let s = &summaries[0];
    assert!(
        !s.investigated.is_empty(),
        "investigated should be non-empty; got {:?}",
        s.investigated
    );
    let _ = std::fs::remove_dir_all(sessions.parent().unwrap());
}

#[test]
fn test_completed_populated_on_test_ok() {
    let (sessions, memory) = tmp_dirs("comp");
    finalize_session(
        &sessions,
        &memory,
        &[r#"{"type":"bash","cmd":"cargo test","in_tk":500,"out_tk":100,"files":[],"errors":[],"git":[],"test_summary":"test result: ok. 35 passed; 0 failed","ts":1774137601}"#],
    );
    let summaries = squeez::memory::read_last_n(&memory, 1);
    assert!(!summaries.is_empty(), "expected a summary");
    let s = &summaries[0];
    assert!(
        !s.completed.is_empty(),
        "completed should be non-empty when tests passed; got {:?}",
        s.completed
    );
    assert!(
        s.completed[0].to_lowercase().contains("ok"),
        "completed should mention ok; got {:?}",
        s.completed
    );
    let _ = std::fs::remove_dir_all(sessions.parent().unwrap());
}

#[test]
fn test_next_steps_populated_on_test_failed() {
    let (sessions, memory) = tmp_dirs("ns");
    finalize_session(
        &sessions,
        &memory,
        &[r#"{"type":"bash","cmd":"cargo test","in_tk":500,"out_tk":150,"files":[],"errors":["error[E0308]: mismatched types at src/filter.rs:42"],"git":[],"test_summary":"test result: FAILED. 30 passed; 5 failed","ts":1774137601}"#],
    );
    let summaries = squeez::memory::read_last_n(&memory, 1);
    assert!(!summaries.is_empty(), "expected a summary");
    let s = &summaries[0];
    assert!(
        !s.next_steps.is_empty(),
        "next_steps should be non-empty when tests failed; got {:?}",
        s.next_steps
    );
    let _ = std::fs::remove_dir_all(sessions.parent().unwrap());
}

#[test]
fn test_learned_populated_from_errors_and_git() {
    let (sessions, memory) = tmp_dirs("lrn");
    finalize_session(
        &sessions,
        &memory,
        &[r#"{"type":"bash","cmd":"cargo build","in_tk":200,"out_tk":80,"files":[],"errors":["error[E0432]: unresolved import at src/main.rs:1"],"git":["abc1234 fix: update deps"],"test_summary":"","ts":1774137601}"#],
    );
    let summaries = squeez::memory::read_last_n(&memory, 1);
    assert!(!summaries.is_empty(), "expected a summary");
    let s = &summaries[0];
    assert!(
        !s.learned.is_empty(),
        "learned should be non-empty when errors/git present; got {:?}",
        s.learned
    );
    let _ = std::fs::remove_dir_all(sessions.parent().unwrap());
}

// ── JSONL round-trip ────────────────────────────────────────────────────────

#[test]
fn test_structured_fields_roundtrip_jsonl() {
    use squeez::memory::{read_last_n, write_summary, Summary};

    let (_, memory) = tmp_dirs("rt");
    let s = Summary {
        date: "2026-04-10".into(),
        duration_min: 15,
        tokens_saved: 5000,
        files_touched: vec!["src/foo.rs".into()],
        files_committed: vec![],
        test_summary: "test result: ok. 10 passed".into(),
        errors_resolved: vec![],
        git_events: vec![],
        ts: 1000,
        valid_from: 1000,
        valid_to: 0,
        investigated: vec!["src/foo.rs".into(), "src/bar.rs".into()],
        learned: vec!["git:abc1234".into()],
        completed: vec!["test result: ok. 10 passed".into()],
        next_steps: vec!["fix E0308 in filter.rs".into()],
        compression_ratio_bp: 0,
        tool_choice_efficiency_bp: 0,
        context_reuse_rate_bp: 0,
        budget_utilization_bp: 0,
        efficiency_overall_bp: 0,
    };
    write_summary(&memory, &s);

    let loaded = read_last_n(&memory, 1);
    assert_eq!(loaded.len(), 1);
    let r = &loaded[0];
    assert_eq!(r.investigated, s.investigated);
    assert_eq!(r.learned, s.learned);
    assert_eq!(r.completed, s.completed);
    assert_eq!(r.next_steps, s.next_steps);
}

#[test]
fn test_old_jsonl_without_structured_fields_loads_with_defaults() {
    let (_, memory) = tmp_dirs("legacy");
    let legacy = "{\"date\":\"2026-01-01\",\"duration_min\":5,\"tokens_saved\":100,\
\"files_touched\":[],\"files_committed\":[],\"test_summary\":\"\",\
\"errors_resolved\":[],\"git_events\":[],\"ts\":500,\"valid_from\":500,\"valid_to\":0}\n";
    std::fs::write(memory.join("summaries.jsonl"), legacy).unwrap();

    let summaries = squeez::memory::read_last_n(&memory, 1);
    assert_eq!(summaries.len(), 1);
    let s = &summaries[0];
    assert!(s.investigated.is_empty(), "investigated should default []");
    assert!(s.learned.is_empty(), "learned should default []");
    assert!(s.completed.is_empty(), "completed should default []");
    assert!(s.next_steps.is_empty(), "next_steps should default []");
}

// ── display_line shows pending count ───────────────────────────────────────

#[test]
fn test_display_line_shows_pending() {
    use squeez::memory::Summary;
    let s = Summary {
        date: "2026-04-10".into(),
        duration_min: 0,
        tokens_saved: 0,
        files_touched: vec!["a.rs".into()],
        files_committed: vec![],
        test_summary: String::new(),
        errors_resolved: vec![],
        git_events: vec![],
        ts: 0,
        valid_from: 0,
        valid_to: 0,
        investigated: vec![],
        learned: vec![],
        completed: vec![],
        next_steps: vec!["fix E0308".into(), "fix E0001".into()],
        compression_ratio_bp: 0,
        tool_choice_efficiency_bp: 0,
        context_reuse_rate_bp: 0,
        budget_utilization_bp: 0,
        efficiency_overall_bp: 0,
    };
    let line = s.display_line();
    assert!(line.contains("2 pending"), "expected '2 pending' in: {}", line);
}

// ── Phase 3: search_history / file_history ─────────────────────────────────

#[test]
fn test_search_history_finds_match() {
    use squeez::memory::{search_history, write_summary, Summary};

    let (_, memory) = tmp_dirs("srch");
    write_summary(
        &memory,
        &Summary {
            date: "2026-04-10".into(),
            duration_min: 10,
            tokens_saved: 1000,
            files_touched: vec!["src/filter.rs".into()],
            files_committed: vec![],
            test_summary: String::new(),
            errors_resolved: vec!["error[E0308]: mismatched types".into()],
            git_events: vec![],
            ts: 1000,
            valid_from: 1000,
            valid_to: 0,
            investigated: vec!["src/filter.rs".into()],
            learned: vec!["error[E0308]: mismatched types".into()],
            completed: vec![],
            next_steps: vec!["error[E0308]: mismatched types".into()],
            compression_ratio_bp: 0,
            tool_choice_efficiency_bp: 0,
            context_reuse_rate_bp: 0,
            budget_utilization_bp: 0,
            efficiency_overall_bp: 0,
        },
    );

    let results = search_history(&memory, "E0308", 10);
    assert!(!results.is_empty(), "search should find E0308");
    assert_eq!(results[0].date, "2026-04-10");
}

#[test]
fn test_search_history_no_match() {
    use squeez::memory::{search_history, write_summary, Summary};

    let (_, memory) = tmp_dirs("srch_none");
    write_summary(
        &memory,
        &Summary {
            date: "2026-04-10".into(),
            duration_min: 5,
            tokens_saved: 0,
            files_touched: vec![],
            files_committed: vec![],
            test_summary: String::new(),
            errors_resolved: vec![],
            git_events: vec![],
            ts: 1000,
            valid_from: 1000,
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
        },
    );

    let results = search_history(&memory, "xyzzy_not_present", 10);
    assert!(results.is_empty(), "should return no results for unknown query");
}

#[test]
fn test_file_history_finds_touched_file() {
    use squeez::memory::{file_history, write_summary, Summary};

    let (_, memory) = tmp_dirs("fhist");
    write_summary(
        &memory,
        &Summary {
            date: "2026-04-10".into(),
            duration_min: 8,
            tokens_saved: 2000,
            files_touched: vec!["src/memory.rs".into(), "src/config.rs".into()],
            files_committed: vec!["src/memory.rs".into()],
            test_summary: String::new(),
            errors_resolved: vec![],
            git_events: vec![],
            ts: 1000,
            valid_from: 1000,
            valid_to: 0,
            investigated: vec!["src/memory.rs".into()],
            learned: vec![],
            completed: vec![],
            next_steps: vec![],
            compression_ratio_bp: 0,
            tool_choice_efficiency_bp: 0,
            context_reuse_rate_bp: 0,
            budget_utilization_bp: 0,
            efficiency_overall_bp: 0,
        },
    );

    let results = file_history(&memory, "memory.rs", 10);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].date, "2026-04-10");
    assert!(results[0].committed, "memory.rs was in files_committed");
    assert_eq!(results[0].tokens_saved, 2000);
}
