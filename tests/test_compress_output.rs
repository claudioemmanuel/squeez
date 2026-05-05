use squeez::commands::compress_output::compute_rewrite;
use squeez::config::Config;
use squeez::context::cache::SessionContext;
use squeez::context::redundancy;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn tmp() -> PathBuf {
    static CTR: AtomicU64 = AtomicU64::new(0);
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "squeez_co_test_{}_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        n
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn cfg() -> Config {
    Config::default()
}

fn make_json(content: &str) -> String {
    // Escape for JSON string embedding
    let escaped = content.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
    format!(r#"{{"tool_name":"Read","tool_result":{{"content":"{escaped}"}}}}"#)
}

// ── passthrough ──────────────────────────────────────────────────────────────

#[test]
fn empty_raw_returns_none() {
    let dir = tmp();
    assert!(compute_rewrite("", "Read", &dir, &cfg()).is_none());
    assert!(compute_rewrite("   ", "Read", &dir, &cfg()).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_content_field_returns_none() {
    let dir = tmp();
    let json = r#"{"tool_name":"Read","tool_result":{}}"#;
    assert!(compute_rewrite(json, "Read", &dir, &cfg()).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn small_novel_content_returns_none_and_records() {
    let dir = tmp();
    let cfg = cfg();
    // 5 distinct lines — small, no summarize, first time seen → passthrough
    let content = "alpha\nbeta\ngamma\ndelta\nepsilon";
    let json = make_json(content);
    assert!(compute_rewrite(&json, "Read", &dir, &cfg).is_none());
    // Should have been recorded in SessionContext
    let ctx = SessionContext::load(&dir);
    assert!(ctx.call_log.len() >= 1, "content should be recorded for future dedup");
    let _ = std::fs::remove_dir_all(&dir);
}

// ── exact redundancy dedup ────────────────────────────────────────────────────

#[test]
fn exact_duplicate_returns_dedup_note() {
    let dir = tmp();
    let cfg = cfg();
    // Need enough lines for redundancy::check to engage (MIN_LINES = 2)
    let content = "line one\nline two\nline three\nline four\nline five";
    let json = make_json(content);

    // First call: novel → None, records
    assert!(compute_rewrite(&json, "Read", &dir, &cfg).is_none());

    // Second call: exact duplicate → Some with dedup note
    let result = compute_rewrite(&json, "Read", &dir, &cfg);
    assert!(result.is_some(), "second call with same content should return a dedup note");
    let note = result.unwrap();
    assert!(
        note.contains("[squeez: identical to Read"),
        "dedup note should identify tool and call: got {note:?}"
    );
    assert!(
        note.contains("— output omitted]"),
        "dedup note should say output omitted: got {note:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn exact_dedup_hit_increments_counter() {
    let dir = tmp();
    let cfg = cfg();
    let content = "a\nb\nc\nd\ne\nf";
    let json = make_json(content);
    compute_rewrite(&json, "Read", &dir, &cfg);
    compute_rewrite(&json, "Read", &dir, &cfg);
    let ctx = SessionContext::load(&dir);
    assert!(ctx.exact_dedup_hits >= 1, "exact_dedup_hits should be incremented");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn different_content_does_not_dedup() {
    let dir = tmp();
    let cfg = cfg();
    let json_a = make_json("apple\nbanana\ncherry\ndate\nelder");
    let json_b = make_json("zebra\nyak\nxray\nwolf\nviper");
    compute_rewrite(&json_a, "Read", &dir, &cfg);
    let result = compute_rewrite(&json_b, "Read", &dir, &cfg);
    assert!(result.is_none(), "distinct content should not trigger dedup");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dedup_note_includes_call_number() {
    let dir = tmp();
    let cfg = cfg();
    // Pre-seed context with a specific call so we know what call_n to expect
    let lines: Vec<String> = (0..10).map(|i| format!("seed line {i}")).collect();
    let mut ctx = SessionContext::load(&dir);
    redundancy::record(&mut ctx, "Read", &lines);
    ctx.save(&dir);

    let content = lines.join("\n");
    let json = make_json(&content);
    let result = compute_rewrite(&json, "Read", &dir, &cfg);
    assert!(result.is_some());
    let note = result.unwrap();
    assert!(note.contains('#'), "dedup note should contain call number with #: {note:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ── summarize large outputs ───────────────────────────────────────────────────

#[test]
fn large_output_is_summarized() {
    let dir = tmp();
    let mut cfg = cfg();
    // Set a very low threshold so we can test with a small fixture
    cfg.summarize_threshold_lines = 10;

    // Use enough lines that the summarizer (≤40 lines out) is definitely shorter.
    // Include an error marker so is_benign() returns false (no benign multiplier).
    let mut lines: Vec<String> = (0..80).map(|i| format!("build output line {i}: compiled ok")).collect();
    lines.push("error: build failed with 1 error".to_string());
    let content = lines.join("\n");
    let json = make_json(&content);

    let result = compute_rewrite(&json, "Read", &dir, &cfg);
    assert!(
        result.is_some(),
        "output exceeding summarize_threshold_lines should be summarized"
    );
    let summary = result.unwrap();
    let summary_lines = summary.lines().count();
    assert!(
        summary_lines < 81,
        "summary ({summary_lines} lines) should be shorter than original (81 lines)"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ── redundancy disabled ───────────────────────────────────────────────────────

#[test]
fn redundancy_disabled_never_deduplicates() {
    let dir = tmp();
    let mut cfg = cfg();
    cfg.redundancy_cache_enabled = false;

    let content = "x\ny\nz\nw\nv";
    let json = make_json(content);
    compute_rewrite(&json, "Read", &dir, &cfg);
    // Even on second call, no dedup when disabled
    let result = compute_rewrite(&json, "Read", &dir, &cfg);
    assert!(result.is_none(), "dedup should not fire when redundancy_cache_enabled=false");
    let _ = std::fs::remove_dir_all(&dir);
}

// ── tool name in note ─────────────────────────────────────────────────────────

#[test]
fn dedup_note_contains_tool_name() {
    let dir = tmp();
    let cfg = cfg();
    let content = "foo\nbar\nbaz\nqux\nquux";
    let json = make_json(content);
    compute_rewrite(&json, "Grep", &dir, &cfg);
    let result = compute_rewrite(&json, "Grep", &dir, &cfg);
    assert!(result.is_some());
    assert!(
        result.unwrap().contains("Grep"),
        "dedup note should name the tool"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
