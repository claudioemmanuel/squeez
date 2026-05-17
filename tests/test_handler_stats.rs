// Integration tests for the continuous handler calibration aggregator.
//
// Unit tests for record/accumulate/sort live in src/economy/handler_stats.rs.
// This file exercises the disk persistence layer end-to-end: cross-session
// accumulation, format_table flagging, and graceful handling of a missing file.

use squeez::economy::handler_stats::{format_table, HandlerStats};

fn tmp_dir(label: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "squeez_hs_it_{}_{}",
        label,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn accumulates_across_simulated_sessions() {
    // Two separate "sessions" record into the same handler_stats.json:
    // the second session reads what the first wrote and increments.
    let dir = tmp_dir("crosssess");

    {
        let mut s = HandlerStats::load(&dir);
        s.record("git", 800, 100);
        s.record("cargo", 4000, 500);
        s.save(&dir);
    }

    {
        let mut s = HandlerStats::load(&dir);
        s.record("git", 600, 50);
        s.record("npm", 200, 180);
        s.save(&dir);
    }

    let final_stats = HandlerStats::load(&dir);
    let rows = final_stats.rows();

    let by_name = |n: &str| rows.iter().find(|r| r.name == n).cloned();
    let git = by_name("git").unwrap();
    assert_eq!(git.calls, 2);
    assert_eq!(git.in_tokens, 1400);
    assert_eq!(git.out_tokens, 150);

    let cargo = by_name("cargo").unwrap();
    assert_eq!(cargo.calls, 1);

    let npm = by_name("npm").unwrap();
    assert_eq!(npm.calls, 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn format_table_lists_under_and_over_performers() {
    let mut s = HandlerStats::default();
    // `bad` — 8 calls, only 5% savings (under-performer).
    for _ in 0..8 {
        s.record("bad", 1000, 950);
    }
    // `good` — 6 calls, 95% savings (over-performer).
    for _ in 0..6 {
        s.record("good", 1000, 50);
    }
    // `mid` — 6 calls, 50% savings (neither).
    for _ in 0..6 {
        s.record("mid", 1000, 500);
    }

    let table = format_table(&s);
    // The high-level table renders all three.
    assert!(table.contains("bad"));
    assert!(table.contains("good"));
    assert!(table.contains("mid"));
    // And it calls out the extremes by name.
    assert!(table.contains("under-performers"));
    assert!(table.contains("over-performers"));
    let under_line = table
        .lines()
        .find(|l| l.starts_with("under-performers"))
        .expect("missing under-performers line");
    assert!(under_line.contains("bad"));
    assert!(!under_line.contains("good"));
    let over_line = table
        .lines()
        .find(|l| l.starts_with("over-performers"))
        .expect("missing over-performers line");
    assert!(over_line.contains("good"));
    assert!(!over_line.contains("bad"));
}

#[test]
fn missing_file_loads_as_empty_stats() {
    let dir = tmp_dir("missing");
    // No prior file written.
    let s = HandlerStats::load(&dir);
    assert!(s.names.is_empty());
    assert!(s.calls.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rows_sorted_by_call_count_desc() {
    let mut s = HandlerStats::default();
    s.record("rare", 10, 1);
    for _ in 0..3 {
        s.record("often", 10, 1);
    }
    s.record("once", 10, 1);

    let rows = s.rows();
    assert_eq!(rows[0].name, "often");
    assert!(rows[0].calls >= rows[1].calls);
    assert!(rows[1].calls >= rows[2].calls);
}

#[test]
fn savings_pct_renders_in_table() {
    let mut s = HandlerStats::default();
    // 10 calls of 80% savings — pct cell should read "80%".
    for _ in 0..10 {
        s.record("eighty", 1000, 200);
    }
    let table = format_table(&s);
    let row_line = table
        .lines()
        .find(|l| l.starts_with("eighty"))
        .expect("expected eighty row");
    assert!(row_line.contains("80%"), "row: {:?}", row_line);
}
