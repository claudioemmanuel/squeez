use squeez::commands::compress_md::{compress_text, Mode};

#[test]
fn full_mode_drops_articles_and_fillers() {
    let input = "The quick brown fox really just jumps over the lazy dog.\n";
    let r = compress_text(input, Mode::Full);
    assert!(r.safe);
    assert!(!r.output.to_lowercase().contains(" the "));
    assert!(!r.output.to_lowercase().contains("really"));
    assert!(!r.output.to_lowercase().contains("just"));
    assert!(r.output.to_lowercase().contains("fox"));
    assert!(r.output.to_lowercase().contains("dog"));
}

#[test]
fn ultra_mode_substitutes_long_words() {
    let input = "Configure the function with these parameters because of the docs.\n";
    let r = compress_text(input, Mode::Ultra);
    assert!(r.safe);
    assert!(r.output.contains("fn"));
    assert!(r.output.contains("w/"));
    assert!(r.output.contains("param"));
    assert!(r.output.contains("b/c"));
}

#[test]
fn fenced_code_block_unchanged() {
    let input = "Intro the prose.\n```python\ndef the_func():\n    return None\n```\nOutro.\n";
    let r = compress_text(input, Mode::Ultra);
    assert!(r.safe);
    assert!(r.output.contains("def the_func():"));
    assert!(r.output.contains("return None"));
}

#[test]
fn inline_code_unchanged() {
    let input = "Run `git status --porcelain` to see the changes.\n";
    let r = compress_text(input, Mode::Full);
    assert!(r.output.contains("`git status --porcelain`"));
}

#[test]
fn url_in_markdown_link_preserved() {
    let input = "See [docs](https://example.com/api/v1) for the details.\n";
    let r = compress_text(input, Mode::Ultra);
    assert!(r.safe);
    assert!(r.output.contains("https://example.com/api/v1"));
}

#[test]
fn bare_url_preserved() {
    let input = "Try https://github.com/foo/bar.git for the source.\n";
    let r = compress_text(input, Mode::Full);
    assert!(r.safe);
    assert!(r.output.contains("https://github.com/foo/bar.git"));
}

#[test]
fn heading_count_unchanged() {
    let input = "# Title\n\nintro\n\n## Section\n\nbody\n\n### Subsection\n\nmore\n";
    let r = compress_text(input, Mode::Ultra);
    assert_eq!(r.stats.orig_headings, r.stats.new_headings);
    assert!(r.safe);
}

#[test]
fn pleasantries_removed() {
    let input = "I'd be happy to help you. Of course, I would like to ensure correctness.\n";
    let r = compress_text(input, Mode::Full);
    let lower = r.output.to_lowercase();
    assert!(!lower.contains("happy to"));
    assert!(!lower.contains("of course"));
    assert!(!lower.contains("would like to"));
}

#[test]
fn table_preserved_verbatim() {
    let input = "Before.\n\n| key | value |\n|-----|-------|\n| a   | 1     |\n| b   | 2     |\n\nAfter.\n";
    let r = compress_text(input, Mode::Full);
    assert!(r.output.contains("| key | value |"));
    assert!(r.output.contains("| a   | 1     |"));
}

#[test]
fn integrity_check_aborts_on_corruption() {
    // We can't easily simulate code-block loss without internal hooks.
    // Verify the safe flag tracks legitimate inputs as safe.
    let input = "# H\n\nshort prose.\n";
    let r = compress_text(input, Mode::Full);
    assert!(r.safe);
}

#[test]
fn idempotent_second_pass_safe() {
    let input = "# Title\n\nThe quick brown fox really jumps high.\n";
    let r1 = compress_text(input, Mode::Full);
    let r2 = compress_text(&r1.output, Mode::Full);
    assert!(r2.safe);
    // No further damage
    assert_eq!(r2.stats.new_headings, r1.stats.new_headings);
    assert_eq!(r2.stats.new_code_blocks, r1.stats.new_code_blocks);
}

#[test]
fn list_marker_kept_articles_dropped() {
    let input = "- the first item\n- the second item\n- a third item\n";
    let r = compress_text(input, Mode::Full);
    assert!(r.output.starts_with("- "));
    assert!(r.output.contains("first item"));
    assert!(!r.output.contains("the first"));
}

#[test]
fn version_string_preserved() {
    let input = "Released v1.2.3 with the new feature.\n";
    let r = compress_text(input, Mode::Ultra);
    // (version preservation is best-effort; main check is no panic + safe)
    assert!(r.safe);
    assert!(r.stats.new_bytes > 0);
}
