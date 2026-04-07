use squeez::context::cache::SessionContext;
use squeez::context::redundancy::{check, record};

fn lines(prefix: &str, n: usize) -> Vec<String> {
    (0..n).map(|i| format!("{}-{}", prefix, i)).collect()
}

#[test]
fn second_identical_run_hits() {
    let mut ctx = SessionContext::default();
    let out = lines("a", 10);
    record(&mut ctx, "git status", &out);
    let hit = check(&ctx, &out);
    assert!(hit.is_some());
    let h = hit.unwrap();
    assert_eq!(h.call_n, 1);
    assert_eq!(h.short_hash.len(), 8);
}

#[test]
fn diff_by_one_line_misses() {
    let mut ctx = SessionContext::default();
    let mut out = lines("a", 10);
    record(&mut ctx, "ls", &out);
    out[0] = "different content".to_string();
    assert!(check(&ctx, &out).is_none());
}

#[test]
fn outside_recent_window_misses() {
    let mut ctx = SessionContext::default();
    let target = lines("first", 10);
    record(&mut ctx, "first", &target);
    // Push 9 more distinct calls (RECENT_WINDOW = 8)
    for i in 0..9 {
        record(&mut ctx, &format!("c{}", i), &lines(&format!("f{}", i), 10));
    }
    assert!(check(&ctx, &target).is_none());
}

#[test]
fn tiny_output_skipped() {
    let mut ctx = SessionContext::default();
    let out = lines("x", 3);
    record(&mut ctx, "echo", &out);
    assert!(check(&ctx, &out).is_none());
}

#[test]
fn record_increments_call_n() {
    let mut ctx = SessionContext::default();
    assert_eq!(record(&mut ctx, "a", &lines("a", 10)), 1);
    assert_eq!(record(&mut ctx, "b", &lines("b", 10)), 2);
    assert_eq!(record(&mut ctx, "c", &lines("c", 10)), 3);
}
