//! `squeez track-filter` (issue #292): output a host compressed via
//! `squeez filter` must reach the session ledger the way `squeez wrap` does.

use std::path::PathBuf;

use squeez::commands::track::run_filter_with;
use squeez::config::Config;
use squeez::session::CurrentSession;

fn tmp_dir(label: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "squeez_track_filter_{}_{}",
        label,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn seed_session(dir: &PathBuf) {
    CurrentSession {
        session_file: "2026-10-30-10.jsonl".to_string(),
        start_ts: 1_000,
        ..Default::default()
    }
    .save(dir);
}

fn big_output() -> String {
    (0..400).map(|i| format!("src/module_{i}.rs | 12 ++++++------\n")).collect()
}

#[test]
fn compressed_output_counts_as_saved() {
    let dir = tmp_dir("saved");
    seed_session(&dir);
    let raw = big_output();
    let emitted = raw.chars().count() / 4;

    run_filter_with("git log --stat", &raw, emitted, &dir, &Config::default());

    let s = CurrentSession::load(&dir).unwrap();
    assert!(s.tokens_saved > 0, "nothing saved: {:?}", s);
    assert!(s.total_tokens > 0 && s.total_tokens < s.tokens_saved);
    let log = std::fs::read_to_string(dir.join("2026-10-30-10.jsonl")).unwrap();
    assert!(log.contains("\"type\":\"bash\""), "{log}");
    assert!(log.contains("\"cmd\":\"git log --stat\""), "{log}");
}

#[test]
fn output_left_as_is_counts_tokens_but_no_savings() {
    let dir = tmp_dir("asis");
    seed_session(&dir);
    let raw = big_output();

    run_filter_with("git log --stat", &raw, raw.chars().count(), &dir, &Config::default());

    let s = CurrentSession::load(&dir).unwrap();
    assert_eq!(s.tokens_saved, 0);
    assert!(s.total_tokens > 0);
}

#[test]
fn no_session_is_a_silent_no_op() {
    let dir = tmp_dir("nosession");
    run_filter_with("ls", &big_output(), 10, &dir, &Config::default());
    assert!(CurrentSession::load(&dir).is_none());
    assert!(std::fs::read_dir(&dir).unwrap().next().is_none());
}
