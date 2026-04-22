use std::io::Write;
use std::process::{Command, Stdio};

fn squeez_bin() -> String {
    format!(
        "{}/target/debug/squeez",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// Pipe `input` into `squeez compress-prompt` and return stdout.
fn compress_prompt(input: &str) -> String {
    let mut child = Command::new(squeez_bin())
        .arg("compress-prompt")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn squeez compress-prompt");

    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();

    let output = child.wait_with_output().expect("failed to wait");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Build a large markdown string (~10 000 chars, well over 2000 tokens).
fn large_markdown() -> String {
    let filler = "This is just really basically a simple description of the feature that the agent should implement completely.\n";
    let mut s = String::from("# Implementation Plan\n\n## Overview\n\n");
    while s.len() < 12_000 {
        s.push_str("### Section\n\n");
        s.push_str(filler);
    }
    s
}

#[test]
fn short_prompt_passes_through_unchanged() {
    let input = "# Short plan\n\nDo one thing.\n";
    let out = compress_prompt(input);
    // Under the default 2000-token budget — output must equal input exactly.
    assert_eq!(out, input, "short prompt should be echoed verbatim");
}

#[test]
fn large_prompt_is_shorter_than_input() {
    let input = large_markdown();
    let out = compress_prompt(&input);
    assert!(
        out.len() < input.len(),
        "compressed output ({} bytes) should be shorter than input ({} bytes)",
        out.len(),
        input.len()
    );
}

#[test]
fn large_prompt_retains_key_headings() {
    let input = large_markdown();
    let out = compress_prompt(&input);
    // The top-level heading must survive (headings are protected verbatim).
    assert!(
        out.contains("# Implementation Plan"),
        "key heading must be present in compressed output"
    );
}
