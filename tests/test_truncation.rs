use squeez::strategies::truncation::{apply, Keep};

#[test]
fn truncates_head() {
    let lines: Vec<String> = (0..100).map(|i| i.to_string()).collect();
    let r = apply(lines, 20, Keep::Head);
    assert_eq!(r.len(), 21);
    assert_eq!(r[0], "0");
    assert!(r[20].contains("80 lines truncated"));
    assert!(r[20].contains("--no-squeez"));
}

#[test]
fn truncates_tail() {
    let lines: Vec<String> = (0..100).map(|i| i.to_string()).collect();
    let r = apply(lines, 20, Keep::Tail);
    assert_eq!(r.len(), 21);
    assert_eq!(r[1], "80"); // first kept line
    assert!(r[0].contains("80 lines truncated"));
}

#[test]
fn no_truncation_under_limit() {
    let lines: Vec<String> = (0..10).map(|i| i.to_string()).collect();
    assert_eq!(apply(lines, 20, Keep::Head).len(), 10);
}
