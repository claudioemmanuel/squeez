//! Per-session context files: concurrent sessions must not clobber each other.
use squeez::context::cache::{
    context_path_for, latest_context_path, prune_session_contexts, SessionContext,
};

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("squeez-ctx-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn keyed_paths_are_distinct_and_unkeyed_is_legacy() {
    let d = tmp("paths");
    assert_eq!(context_path_for(&d, None), d.join("context.json"));
    assert_ne!(context_path_for(&d, Some("a")), context_path_for(&d, Some("b")));
    assert!(context_path_for(&d, Some("a")).starts_with(d.join("ctx")));
}

#[test]
fn two_sessions_keep_their_own_counters() {
    let d = tmp("iso");
    // History written by an older, unkeyed squeez.
    let mut g = SessionContext::default();
    g.call_counter = 40;
    g.seen_git_refs.push("abc1234".into());
    g.tokens_bash = 999;
    g.save_keyed(&d, None);

    // Session A starts: inherits history, not the old session's totals.
    let mut a = SessionContext::load_keyed(&d, Some("A"));
    assert_eq!(a.seen_git_refs, vec!["abc1234".to_string()]);
    assert_eq!(a.tokens_bash, 0, "totals belong to the session that made them");
    assert_eq!(a.dedup_floor_call, 40, "older output is not in A's context");
    a.tokens_bash = 100;
    a.call_counter += 5;
    a.save_keyed(&d, Some("A"));
    let a_epoch = a.session_epoch;

    // Session B starts while A is alive (seeded from A's newer file).
    let mut b = SessionContext::load_keyed(&d, Some("B"));
    assert_eq!(b.tokens_bash, 0);
    b.tokens_bash = 7;
    b.save_keyed(&d, Some("B"));

    // A is untouched by B's start: totals, floor and epoch.
    let a2 = SessionContext::load_keyed(&d, Some("A"));
    assert_eq!(a2.tokens_bash, 100);
    assert_eq!(a2.dedup_floor_call, 40);
    assert_eq!(a2.session_epoch, a_epoch, "A's in-flight wrap save must not be superseded");
    assert_eq!(SessionContext::load_keyed(&d, Some("B")).tokens_bash, 7);
    // The legacy file was only read, never rewritten.
    assert_eq!(SessionContext::load_keyed(&d, None).tokens_bash, 999);
}

#[test]
fn latest_and_prune() {
    let d = tmp("prune");
    SessionContext::default().save_keyed(&d, None);
    std::thread::sleep(std::time::Duration::from_millis(20));
    SessionContext::default().save_keyed(&d, Some("new"));
    assert_eq!(latest_context_path(&d), context_path_for(&d, Some("new")));
    assert_eq!(prune_session_contexts(&d, 3600), 0, "fresh files stay");
    assert_eq!(prune_session_contexts(&d, 0), 1);
    assert_eq!(latest_context_path(&d), d.join("context.json"));
}
