use squeez::commands::benchmark;

#[test]
fn hypothesis_grid_has_seven_ids() {
    let grid = benchmark::run_hypothesis_grid();
    let ids: Vec<&str> = grid.iter().map(|r| r.id).collect();
    for expected in &["C0", "C1", "C2", "C3", "C4", "C5", "C6"] {
        assert!(
            ids.contains(expected),
            "missing hypothesis ID {} in grid",
            expected
        );
    }
    assert_eq!(grid.len(), 7, "expected exactly 7 hypothesis results");
}

#[test]
fn c0_reduction_pct_is_zero() {
    let grid = benchmark::run_hypothesis_grid();
    let c0 = grid.iter().find(|r| r.id == "C0").expect("C0 missing");
    assert!(
        (c0.reduction_pct - 0.0).abs() < 1e-6,
        "C0 reduction_pct must be 0.0, got {}",
        c0.reduction_pct
    );
    assert_eq!(
        c0.baseline_tokens, c0.compressed_tokens,
        "C0 baseline_tokens must equal compressed_tokens"
    );
}

#[test]
fn c6_has_lowest_compressed_tokens() {
    let grid = benchmark::run_hypothesis_grid();
    let c6 = grid.iter().find(|r| r.id == "C6").expect("C6 missing");
    for r in &grid {
        if r.id == "C0" || r.id == "C6" {
            continue;
        }
        assert!(
            c6.compressed_tokens <= r.compressed_tokens,
            "C6 compressed_tokens ({}) must be <= {} compressed_tokens ({})",
            c6.compressed_tokens,
            r.id,
            r.compressed_tokens
        );
    }
}

#[test]
fn hypothesis_json_contains_schema_version_and_all_ids() {
    let grid = benchmark::run_hypothesis_grid();
    let json = benchmark::hypothesis_to_json(&grid);

    assert!(
        json.contains("\"schema_version\":1"),
        "JSON must contain schema_version:1, got: {}",
        &json[..json.len().min(200)]
    );

    for id in &["C0", "C1", "C2", "C3", "C4", "C5", "C6"] {
        assert!(
            json.contains(&format!("\"id\":\"{}\"", id)),
            "JSON missing id {}: {}",
            id,
            &json[..json.len().min(400)]
        );
    }

    // Basic structural check: opening brace count should indicate an object with array.
    let open_braces = json.chars().filter(|&c| c == '{').count();
    assert!(
        open_braces >= 8,
        "expected at least 8 opening braces (1 root + 7 entries), got {}",
        open_braces
    );
}
