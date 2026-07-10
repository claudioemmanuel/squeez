// Integration tests for the R1 factsheet: deterministic extraction of
// precision-critical identifiers (git SHAs, UUIDs, versions, ticket codes,
// large integers) and their preservation when summarize.rs collapses a large
// output — the dropped head region must not silently lose exact ids.

use squeez::context::factsheet::{extract, MAX_FACTS};
use squeez::context::summarize::{apply_with_format, SummaryFormat};

#[test]
fn extracts_all_identifier_classes() {
    let text = "deploy v1.34.6 commit a1347daf9b2c41e0 for PROJ-1482 \
                trace 550e8400-e29b-41d4-a716-446655440000 build 20260710";
    let facts = extract(text);
    assert!(facts.contains(&"a1347daf9b2c41e0".to_string()));
    assert!(facts.contains(&"550e8400-e29b-41d4-a716-446655440000".to_string()));
    assert!(facts.contains(&"v1.34.6".to_string()));
    assert!(facts.contains(&"PROJ-1482".to_string()));
    assert!(facts.contains(&"20260710".to_string()));
}

#[test]
fn output_is_deterministic_and_tier_ordered() {
    let text = "v2.0.1 PROJ-7 abc1234def 550e8400-e29b-41d4-a716-446655440000";
    let a = extract(text);
    let b = extract(text);
    assert_eq!(a, b);
    // Tier 0 (UUID, hex) first, then ticket, then version.
    assert_eq!(
        a,
        vec![
            "550e8400-e29b-41d4-a716-446655440000".to_string(),
            "abc1234def".to_string(),
            "PROJ-7".to_string(),
            "v2.0.1".to_string(),
        ]
    );
}

#[test]
fn budgets_are_enforced() {
    // Many distinct ids → never more than MAX_FACTS, never over 256 joined chars.
    let text: String = (0..100).map(|i| format!("abcd{:05} ", i)).collect();
    let facts = extract(&text);
    assert!(facts.len() <= MAX_FACTS);
    assert!(facts.join(" ").len() <= 256);
}

#[test]
fn prose_summary_preserves_ids_from_dropped_head() {
    // 600-line output: SHA + UUID in the head (dropped region), noise after.
    let mut lines: Vec<String> = vec![
        "pushed commit a1347daf9b2c41e0aa55 to origin".to_string(),
        "session 550e8400-e29b-41d4-a716-446655440000 started".to_string(),
    ];
    lines.extend((0..598).map(|i| format!("compiling unit {}", i)));

    let out = apply_with_format(lines, "cargo build", SummaryFormat::Prose);
    let joined = out.join("\n");
    assert!(joined.contains("ids_preserved:"), "missing ids_preserved section:\n{}", joined);
    assert!(joined.contains("a1347daf9b2c41e0aa55"));
    assert!(joined.contains("550e8400-e29b-41d4-a716-446655440000"));
    // The ≤40-line bound still holds with the factsheet included.
    assert!(out.len() <= 40, "got {} lines", out.len());
}

#[test]
fn structured_summary_carries_ids_array() {
    let mut lines: Vec<String> = vec![
        "commit a1347daf9b2c41e0aa55".to_string(),
        "ticket PROJ-1482 resolved".to_string(),
    ];
    lines.extend((0..598).map(|i| format!("step {}", i)));

    let out = apply_with_format(lines, "make", SummaryFormat::Structured);
    assert!(out[0].contains("\"ids\":[\"a1347daf9b2c41e0aa55\",\"PROJ-1482\"]"), "{}", out[0]);
}

#[test]
fn structured_summary_ids_empty_when_none() {
    let lines: Vec<String> = (0..600).map(|i| format!("plain text {}", i)).collect();
    let out = apply_with_format(lines, "make", SummaryFormat::Structured);
    assert!(out[0].contains("\"ids\":[]"), "{}", out[0]);
}

#[test]
fn prose_summary_omits_section_when_no_ids() {
    let lines: Vec<String> = (0..600).map(|i| format!("plain text {}", i)).collect();
    let out = apply_with_format(lines, "make", SummaryFormat::Prose);
    assert!(!out.join("\n").contains("ids_preserved:"));
}

#[test]
fn ids_in_tail_only_are_not_duplicated_into_factsheet() {
    // Identifier appears only in the preserved tail → it survives verbatim
    // there, so the factsheet must stay empty.
    let mut lines: Vec<String> = (0..599).map(|i| format!("quiet {}", i)).collect();
    lines.push("done at commit a1347daf9b2c41e0aa55".to_string());
    let out = apply_with_format(lines, "make", SummaryFormat::Prose);
    let joined = out.join("\n");
    assert!(!joined.contains("ids_preserved:"));
    assert!(joined.contains("a1347daf9b2c41e0aa55")); // still present via tail
}
