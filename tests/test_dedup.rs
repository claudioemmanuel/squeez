use squeez::strategies::dedup::apply;

#[test]
fn collapses_repeated_lines() {
    let input = vec!["error: connection refused".to_string(); 5];
    let result = apply(input, 3);
    assert_eq!(result.len(), 1);
    assert!(result[0].contains("[×5]"));
}

#[test]
fn keeps_lines_below_threshold() {
    let input = vec!["warning: foo".to_string(), "warning: foo".to_string()];
    assert_eq!(apply(input, 3).len(), 2);
}

#[test]
fn preserves_unique_order() {
    let input = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    assert_eq!(apply(input, 3), vec!["a", "b", "c"]);
}

#[test]
fn empty_input() {
    assert!(apply(vec![], 3).is_empty());
}

#[test]
fn exactly_at_threshold_collapses() {
    // Exactly dedup_min=3 repetitions should collapse
    let input = vec!["warn: timeout".to_string(); 3];
    let result = apply(input, 3);
    assert_eq!(result.len(), 1);
    assert!(result[0].contains("[×3]"));
}

#[test]
fn one_below_threshold_preserved() {
    // dedup_min-1 = 2 repetitions should NOT collapse when threshold is 3
    let input = vec!["warn: timeout".to_string(); 2];
    let result = apply(input, 3);
    assert_eq!(result.len(), 2, "below threshold should not collapse");
}
