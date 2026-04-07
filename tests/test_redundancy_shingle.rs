// Integration tests for the fuzzy (shingle-Jaccard) branch of redundancy
// matching, added in addition to the existing exact-hash tests in
// tests/test_redundancy.rs. These verify that whitespace differences,
// single-line edits, and timestamp drift no longer defeat the dedup, while
// genuinely different outputs are still rejected.

use squeez::context::cache::SessionContext;
use squeez::context::redundancy::{check, record};

fn lines_token(prefix: &str, n: usize) -> Vec<String> {
    // Each line gets enough tokens that whitespace differences move multiple
    // trigrams when collapsed. Per-line content is "{prefix} word{i} stable
    // tail token alpha beta gamma {i}".
    (0..n)
        .map(|i| {
            format!(
                "{} word{} stable tail token alpha beta gamma {}",
                prefix, i, i
            )
        })
        .collect()
}

#[test]
fn whitespace_only_diff_matches_via_similarity() {
    let mut ctx = SessionContext::default();
    let original: Vec<String> = (0..15)
        .map(|i| format!("file{}.rs   modified  status  ok  size{}", i, i * 10))
        .collect();
    record(&mut ctx, "git status", &original);
    let with_diff_ws: Vec<String> = (0..15)
        .map(|i| format!("file{}.rs modified status ok size{}", i, i * 10))
        .collect();
    let hit = check(&ctx, &with_diff_ws).expect("whitespace-only diff should still match");
    assert!(
        hit.similarity.is_some(),
        "should be a similarity hit, not exact"
    );
    let s = hit.similarity.unwrap();
    assert!(s >= 0.85, "expected high similarity, got {}", s);
}

#[test]
fn single_line_edit_in_long_output_still_matches() {
    let mut ctx = SessionContext::default();
    let original = lines_token("a", 30);
    record(&mut ctx, "long", &original);
    let mut modified = original.clone();
    modified[10] = "completely different content here totally unrelated".to_string();
    let hit = check(&ctx, &modified).expect("single-line edit in 30 lines should still match");
    assert!(
        hit.similarity.is_some(),
        "should be a similarity hit (one-line edit defeats exact)"
    );
}

#[test]
fn unrelated_outputs_dont_match_via_similarity() {
    let mut ctx = SessionContext::default();
    let a = lines_token("apple", 20);
    record(&mut ctx, "first", &a);
    let b: Vec<String> = (0..20)
        .map(|i| format!("totally different unrelated entirely something else {}", i))
        .collect();
    assert!(
        check(&ctx, &b).is_none(),
        "unrelated content should not trigger similarity match"
    );
}

#[test]
fn length_ratio_guard_rejects_huge_size_difference() {
    let mut ctx = SessionContext::default();
    // Same vocabulary but 10x more content — guard should reject.
    let small: Vec<String> = (0..8).map(|i| format!("token alpha beta {}", i)).collect();
    record(&mut ctx, "small", &small);
    let big: Vec<String> = (0..120)
        .map(|i| format!("token alpha beta {}", i))
        .collect();
    assert!(
        check(&ctx, &big).is_none(),
        "length ratio guard (>5x size diff) should reject the match"
    );
}

#[test]
fn similar_hit_carries_similarity_field() {
    let mut ctx = SessionContext::default();
    let a = lines_token("a", 25);
    record(&mut ctx, "a", &a);
    let mut b = a.clone();
    // Touch one line so the exact hash misses but Jaccard stays high.
    b[5] = "a word5 STABLE TAIL token alpha beta gamma 5".to_string();
    let hit = check(&ctx, &b).expect("near-identical should match");
    assert!(
        hit.similarity.is_some(),
        "should carry similarity, not be exact"
    );
    let s = hit.similarity.unwrap();
    assert!(
        s >= 0.85 && s <= 1.0,
        "similarity should be in [0.85, 1.0], got {}",
        s
    );
}

#[test]
fn exact_hit_has_no_similarity_field() {
    let mut ctx = SessionContext::default();
    let out = lines_token("x", 12);
    record(&mut ctx, "exact", &out);
    let hit = check(&ctx, &out).expect("exact repeat should hit");
    assert!(
        hit.similarity.is_none(),
        "exact hit should not carry a similarity score"
    );
}

#[test]
fn fuzzy_disabled_for_short_outputs() {
    // MIN_LINES_FUZZY=6 in src/context/redundancy.rs. A 5-line near-match
    // must still NOT trigger the fuzzy branch (only exact-hash applies).
    let mut ctx = SessionContext::default();
    let original = lines_token("p", 5);
    record(&mut ctx, "short", &original);
    let mut modified = original.clone();
    modified[2] = "p word2 different tail token alpha beta gamma 2".to_string();
    assert!(
        check(&ctx, &modified).is_none(),
        "5-line near-match must not trigger fuzzy branch"
    );
}

#[test]
fn json_round_trip_preserves_shingle_match() {
    // Confirm that shingles persist across save/load via to_json/from_json
    // so that the fuzzy branch survives a process restart.
    let mut ctx = SessionContext::default();
    let original = lines_token("rt", 18);
    record(&mut ctx, "round trip", &original);

    let json = ctx.to_json();
    let mut loaded = SessionContext::from_json(&json);

    let mut modified = original.clone();
    modified[7] = "rt word7 EDITED tail token alpha beta gamma 7".to_string();
    let hit = check(&loaded, &modified).expect("loaded ctx should still match via shingles");
    assert!(hit.similarity.is_some());

    // And the loaded context can record new calls without panicking.
    record(&mut loaded, "next", &modified);
}
