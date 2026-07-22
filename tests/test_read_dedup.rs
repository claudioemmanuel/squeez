//! Session-long, path-keyed dedup of repeated `Read` output, plus the dedup
//! floor that keeps prior-session and pre-compaction calls from being cited.

use squeez::commands::compress_output::compute_rewrite;
use squeez::config::Config;
use squeez::context::cache::{FileAccess, SessionContext};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

fn tmp() -> PathBuf {
    static CTR: AtomicU64 = AtomicU64::new(0);
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "squeez_rd_test_{}_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        n
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A Read payload for `path` whose body is `n` distinct lines.
fn read_json(path: &str, n: usize) -> String {
    let body: Vec<String> = (0..n).map(|i| format!("line {i} of {path}")).collect();
    let escaped = body.join("\\n");
    format!(r#"{{"tool_name":"Read","file_path":"{path}","tool_result":{{"content":"{escaped}"}}}}"#)
}

/// Push `n` unrelated Read calls through, to move past `recent_window` (16).
fn noise(dir: &Path, cfg: &Config, n: usize) {
    for i in 0..n {
        let _ = compute_rewrite(&read_json(&format!("/tmp/noise{i}.rs"), 8), "Read", dir, cfg);
    }
}

#[test]
fn reread_collapses_far_past_the_recent_window() {
    let dir = tmp();
    let cfg = Config::default();
    let payload = read_json("/tmp/target.rs", 12);

    assert!(
        compute_rewrite(&payload, "Read", &dir, &cfg).is_none(),
        "first read must pass through untouched"
    );
    noise(&dir, &cfg, 25); // well past recent_window = 16

    let out = compute_rewrite(&payload, "Read", &dir, &cfg).expect("re-read should collapse");
    assert!(out.contains("identical to Read #"), "got: {out}");
    assert!(out.contains("/tmp/target.rs"), "marker must name the file: {out}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn changed_content_at_same_path_does_not_collapse() {
    let dir = tmp();
    let cfg = Config::default();
    compute_rewrite(&read_json("/tmp/t.rs", 12), "Read", &dir, &cfg);
    noise(&dir, &cfg, 25);
    // Same path, different body → different fingerprint → full output.
    assert!(compute_rewrite(&read_json("/tmp/t.rs", 13), "Read", &dir, &cfg).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn same_content_at_a_different_path_does_not_collapse() {
    let dir = tmp();
    let cfg = Config::default();
    // Bodies embed the path, so build one payload and retarget only file_path.
    let body = read_json("/tmp/a.rs", 12);
    let renamed = body.replace(r#""file_path":"/tmp/a.rs""#, r#""file_path":"/tmp/b.rs""#);
    compute_rewrite(&body, "Read", &dir, &cfg);
    noise(&dir, &cfg, 25);
    let out = compute_rewrite(&renamed, "Read", &dir, &cfg);
    assert!(
        out.is_none(),
        "identical bytes at another path is a different answer: {out:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn write_to_the_path_invalidates_the_entry() {
    let dir = tmp();
    let cfg = Config::default();
    let payload = read_json("/tmp/edited.rs", 12);
    compute_rewrite(&payload, "Read", &dir, &cfg);
    noise(&dir, &cfg, 25);

    // An Edit/Write on the path clears the store entry (A2 stale-state guard).
    let mut ctx = SessionContext::load(&dir);
    ctx.note_file("/tmp/edited.rs", FileAccess::Write);
    ctx.save(&dir);

    let out = compute_rewrite(&payload, "Read", &dir, &cfg);
    assert!(out.is_none(), "post-write read must reach the model: {out:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn config_switch_disables_the_store() {
    let dir = tmp();
    let cfg = Config::from_str("read_dedup_session_long = false\n");
    let payload = read_json("/tmp/off.rs", 12);
    compute_rewrite(&payload, "Read", &dir, &cfg);
    noise(&dir, &cfg, 25);
    assert!(compute_rewrite(&payload, "Read", &dir, &cfg).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

// ── dedup floor ──────────────────────────────────────────────────────────────

#[test]
fn floor_blocks_read_store_lookups() {
    let mut c = SessionContext::default();
    c.next_call_n();
    c.read_dedup_record("/tmp/x.rs", 42, c.call_counter);
    assert_eq!(c.read_dedup_lookup("/tmp/x.rs", 42), Some(1));

    c.dedup_floor_call = c.call_counter; // session change or /compact
    assert_eq!(
        c.read_dedup_lookup("/tmp/x.rs", 42),
        None,
        "an entry at or below the floor is no longer in the model's context"
    );
}

#[test]
fn floor_blocks_windowed_and_skill_lookups() {
    let mut c = SessionContext::default();
    c.next_call_n();
    c.record_call("cat x", 111, 20, c.call_counter);
    c.skill_dedup_record(7);
    assert!(c.lookup_recent(111, 20).is_some());
    assert!(c.skill_dedup_lookup(7).is_some());

    c.dedup_floor_call = c.call_counter;
    assert!(c.lookup_recent(111, 20).is_none(), "windowed dedup ignores the floor");
    assert!(c.skill_dedup_lookup(7).is_none(), "skill dedup ignores the floor");
}

#[test]
fn floor_and_store_survive_the_json_round_trip() {
    let dir = tmp();
    let mut c = SessionContext::default();
    c.next_call_n();
    c.read_dedup_record("/tmp/rt.rs", 99, c.call_counter);
    c.dedup_floor_call = 3;
    c.save(&dir);

    let back = SessionContext::load(&dir);
    assert_eq!(back.dedup_floor_call, 3);
    assert_eq!(back.read_dedup_path, vec!["/tmp/rt.rs".to_string()]);
    assert_eq!(back.read_dedup_fp, vec![99]);
    assert_eq!(back.read_dedup_call, vec![1]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn legacy_context_json_loads_without_a_floor() {
    let dir = tmp();
    std::fs::write(
        dir.join("context.json"),
        r#"{"session_file":"s.jsonl","call_counter":5}"#,
    )
    .unwrap();
    let c = SessionContext::load(&dir);
    assert_eq!(c.dedup_floor_call, 0, "legacy load must not fence off dedup");
    assert!(c.read_dedup_path.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn store_is_capped_and_evicts_oldest() {
    let mut c = SessionContext::default();
    for i in 0..300u64 {
        c.next_call_n();
        c.read_dedup_record(&format!("/tmp/f{i}.rs"), i, c.call_counter);
    }
    assert!(c.read_dedup_path.len() <= 256);
    assert_eq!(c.read_dedup_path.len(), c.read_dedup_fp.len());
    assert_eq!(c.read_dedup_path.len(), c.read_dedup_call.len());
    assert_eq!(c.read_dedup_lookup("/tmp/f0.rs", 0), None, "oldest evicted");
    assert!(c.read_dedup_lookup("/tmp/f299.rs", 299).is_some());
}

// ── floor is raised by the real entry points ────────────────────────────────

#[test]
fn session_start_raises_the_floor_and_stamps_the_session() {
    let dir = tmp();
    let memory = dir.join("memory");
    std::fs::create_dir_all(&memory).unwrap();
    let cfg = Config::default();

    // A first session records a read, then ends.
    compute_rewrite(&read_json("/tmp/prior.rs", 12), "Read", &dir, &cfg);
    let before = SessionContext::load(&dir);
    assert!(before.call_counter > 0);
    assert!(before.read_dedup_lookup("/tmp/prior.rs", before.read_dedup_fp[0]).is_some());

    squeez::commands::init::run_with_dirs(&dir, &memory, &cfg);

    let after = SessionContext::load(&dir);
    assert_eq!(after.dedup_floor_call, before.call_counter, "floor raised to the old counter");
    assert!(!after.session_file.is_empty(), "session stamped");
    assert!(
        !after.seen_files.is_empty() || before.seen_files.is_empty(),
        "history must survive the session change"
    );
    // The prior session's read no longer collapses.
    assert!(compute_rewrite(&read_json("/tmp/prior.rs", 12), "Read", &dir, &cfg).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn precompact_raises_the_floor() {
    let dir = tmp();
    let memory = dir.join("memory");
    std::fs::create_dir_all(&memory).unwrap();
    let cfg = Config::default();
    squeez::commands::init::run_with_dirs(&dir, &memory, &cfg);

    let payload = read_json("/tmp/compacted.rs", 12);
    compute_rewrite(&payload, "Read", &dir, &cfg);
    let counter = SessionContext::load(&dir).call_counter;

    squeez::commands::track::run_with_dir("PreCompact", "4000", &dir);

    let ctx = SessionContext::load(&dir);
    assert_eq!(ctx.dedup_floor_call, counter);
    assert!(
        compute_rewrite(&payload, "Read", &dir, &cfg).is_none(),
        "content dropped by compaction must be re-sent in full"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
