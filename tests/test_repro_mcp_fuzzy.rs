// Regression: squeez was dropping distinct browser (chrome-devtools) MCP
// snapshots as redundant, forcing the model to work around it by dumping
// snapshots to a file to read the real element uids.
//
// Two independent defects combined:
//   1. extract_string_field truncated JSON-escaped content at the first `\"`,
//      collapsing every DOM snapshot to its constant pre-quote prefix → exact
//      dedup fired across genuinely different snapshots.
//   2. Fuzzy (~similar) dedup applied to MCP results, where the diff between two
//      near-identical snapshots IS the payload the model asked for.

use squeez::commands::compress_output::compute_rewrite;
use squeez::config::Config;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn tmp() -> PathBuf {
    static CTR: AtomicU64 = AtomicU64::new(0);
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "squeez_repro_{}_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        n
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

const TOOL: &str = "mcp__chrome-devtools__take_snapshot";

/// A PostToolUse payload whose content is a JSON-escaped string, exactly as
/// Claude Code delivers it — inner attribute quotes arrive as `\"`.
fn pt(content_escaped: &str) -> String {
    format!(r#"{{"tool_name":"{TOOL}","tool_result":{{"content":"{content_escaped}"}}}}"#)
}

/// DOM snapshot with escaped quotes; `uid` seeds the element ids (what the
/// model needs). `extra` appends a trailing distinguishing value.
fn snapshot(uid: &str, cells: usize, extra: &str) -> String {
    let mut s = String::from("<body>\\n  <div class=\\\"grid\\\">");
    for c in 0..cells {
        s.push_str(&format!(
            "\\n    uid={uid}_{c} <button class=\\\"cell\\\">Cell {c}</button>"
        ));
    }
    s.push_str(&format!("\\n    <span id=\\\"total\\\">{extra}</span>"));
    s.push_str("\\n  </div>\\n</body>");
    s
}

#[test]
fn escaped_snapshot_differing_in_uids_not_dropped() {
    let dir = tmp();
    let cfg = Config::default();
    compute_rewrite(&pt(&snapshot("1", 8, "x")), TOOL, &dir, &cfg);
    // Different uids — extraction must see past the escaped quotes so the hash
    // differs and this survives.
    let second = compute_rewrite(&pt(&snapshot("2", 8, "x")), TOOL, &dir, &cfg);
    assert!(
        second.is_none(),
        "distinct snapshot dropped by dedup: {second:?}"
    );
}

#[test]
fn near_identical_snapshot_not_fuzzy_dropped() {
    let dir = tmp();
    let cfg = Config::default();
    // 30 identical cells, only the trailing value differs — ~96% similar, but
    // that value is the payload. Fuzzy dedup must not swallow it.
    compute_rewrite(&pt(&snapshot("n", 30, "score: 41")), TOOL, &dir, &cfg);
    let second = compute_rewrite(&pt(&snapshot("n", 30, "score: 99")), TOOL, &dir, &cfg);
    assert!(
        second.is_none(),
        "near-identical MCP snapshot fuzzy-dropped, losing the changed value: {second:?}"
    );
}

#[test]
fn byte_identical_mcp_still_dedups() {
    let dir = tmp();
    let cfg = Config::default();
    let snap = pt(&snapshot("1", 8, "x"));
    compute_rewrite(&snap, TOOL, &dir, &cfg);
    // Exact re-emission carries no new information — still worth collapsing.
    let again = compute_rewrite(&snap, TOOL, &dir, &cfg);
    assert!(
        again.as_deref().map(|s| s.contains("identical")).unwrap_or(false),
        "byte-identical MCP output should still exact-dedup: {again:?}"
    );
}
