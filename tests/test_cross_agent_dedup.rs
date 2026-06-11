// Tests for cross-agent duplicate file-read detection (note_subagent_files).
// Verifies that sibling agents reading the same files are flagged while
// within-agent re-reads and empty inputs are handled gracefully.

use squeez::context::cache::SessionContext;

#[test]
fn different_agents_same_file_is_dup() {
    let mut ctx = SessionContext::default();
    let files_a = vec![
        "/src/orm.py".to_string(),
        "/src/models.py".to_string(),
    ];
    let files_b = vec!["/src/orm.py".to_string()];

    let dups_a = ctx.note_subagent_files("agent-aaa", &files_a);
    assert!(dups_a.is_empty(), "first agent has no prior agents to dup");

    let dups_b = ctx.note_subagent_files("agent-bbb", &files_b);
    assert_eq!(dups_b.len(), 1);
    assert_eq!(dups_b[0], "/src/orm.py");
}

#[test]
fn same_agent_same_file_not_dup() {
    let mut ctx = SessionContext::default();
    let files = vec!["/src/config.py".to_string()];
    ctx.note_subagent_files("agent-aaa", &files);
    // Same agent reads same file again — no dup
    let dups = ctx.note_subagent_files("agent-aaa", &files);
    assert!(dups.is_empty(), "same agent re-reading own file is not a cross-agent dup");
}

#[test]
fn empty_agent_id_is_noop() {
    let mut ctx = SessionContext::default();
    let files = vec!["/src/main.rs".to_string()];
    let dups = ctx.note_subagent_files("", &files);
    assert!(dups.is_empty());
    assert!(ctx.subagent_file_map_ids.is_empty());
}

#[test]
fn empty_paths_is_noop() {
    let mut ctx = SessionContext::default();
    let dups = ctx.note_subagent_files("agent-aaa", &[]);
    assert!(dups.is_empty());
    assert!(ctx.subagent_file_map_ids.is_empty());
}

#[test]
fn map_caps_at_16_entries() {
    let mut ctx = SessionContext::default();
    for i in 0..20u32 {
        let agent_id = format!("agent-{:04}", i);
        let files = vec![format!("/src/file{}.py", i)];
        ctx.note_subagent_files(&agent_id, &files);
    }
    assert!(
        ctx.subagent_file_map_ids.len() <= 16,
        "map should be capped at 16, got {}",
        ctx.subagent_file_map_ids.len()
    );
}

#[test]
fn multiple_dups_reported() {
    let mut ctx = SessionContext::default();
    let files_a = vec![
        "/src/a.py".to_string(),
        "/src/b.py".to_string(),
        "/src/c.py".to_string(),
    ];
    ctx.note_subagent_files("agent-aaa", &files_a);

    let files_b = vec![
        "/src/a.py".to_string(),
        "/src/b.py".to_string(),
        "/src/d.py".to_string(), // new file, not a dup
    ];
    let dups = ctx.note_subagent_files("agent-bbb", &files_b);
    assert_eq!(dups.len(), 2);
    assert!(dups.contains(&"/src/a.py".to_string()));
    assert!(dups.contains(&"/src/b.py".to_string()));
    assert!(!dups.contains(&"/src/d.py".to_string()));
}

#[test]
fn json_roundtrip_preserves_map() {
    let mut ctx = SessionContext::default();
    ctx.note_subagent_files(
        "agent-aaa",
        &["/src/orm.py".to_string(), "/src/models.py".to_string()],
    );
    let restored = SessionContext::from_json(&ctx.to_json());
    // After roundtrip, the map should still detect dups for a new agent
    let dups = restored
        .subagent_file_map_ids
        .iter()
        .zip(restored.subagent_file_map_paths.iter())
        .any(|(id, paths)| id == "agent-aaa" && paths.contains("orm.py"));
    assert!(dups, "subagent_file_map should survive JSON roundtrip");
}
