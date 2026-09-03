// File-level coverage for context-window detection (#199, #219).
//
// The in-module tests cover the text scanners; these exercise `detect_window`
// against a real file, including the head fallback that only triggers once a
// transcript outgrows the 512 KB tail read.

use std::io::Write;
use std::path::PathBuf;

use squeez::context::transcript::detect_window;

/// The model-identity record Claude Code writes at session start. Its
/// `modelId` keeps the `[1m]` suffix that `message.model` drops.
fn identity(model_id: &str, marketing: &str) -> String {
    format!(
        r#"{{"isSidechain":false,"attachment":{{"type":"model","identity":{{"modelId":"{}","marketingName":"{}","knowledgeCutoff":"May 2026"}}}},"type":"attachment"}}"#,
        model_id, marketing
    )
}

/// An ordinary assistant turn. On a 1M session this still records the bare
/// model id, which is exactly why the identity record is needed.
fn assistant(model_id: &str) -> String {
    format!(
        r#"{{"type":"assistant","message":{{"model":"{}","usage":{{"input_tokens":10,"cache_read_input_tokens":1000}}}}}}"#,
        model_id
    )
}

fn write_transcript(name: &str, lines: &[String]) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("squeez_test_{}_{}.jsonl", name, std::process::id()));
    let mut f = std::fs::File::create(&path).expect("create transcript");
    for l in lines {
        writeln!(f, "{}", l).expect("write line");
    }
    path
}

#[test]
fn sonnet_1m_session_detects_1m_not_the_200k_floor() {
    // #219 verbatim: `claude-sonnet-5` on the 1M window. Every assistant record
    // says plain `claude-sonnet-5`; only the identity record proves the tier.
    let mut lines = vec![identity("claude-sonnet-5[1m]", "Sonnet 5 (1M context)")];
    for _ in 0..50 {
        lines.push(assistant("claude-sonnet-5"));
    }
    let path = write_transcript("sonnet_1m", &lines);
    assert_eq!(detect_window(&path), Some(1_000_000));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn sonnet_standard_session_stays_at_200k() {
    let lines = vec![
        identity("claude-sonnet-5", "Sonnet 5"),
        assistant("claude-sonnet-5"),
    ];
    let path = write_transcript("sonnet_std", &lines);
    assert_eq!(detect_window(&path), Some(200_000));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn identity_at_the_head_survives_a_transcript_longer_than_the_tail_read() {
    // The identity record is written once, at the top. A long session pushes it
    // far outside the 512 KB tail, so detection has to fall back to the head.
    let mut lines = vec![identity("claude-sonnet-5[1m]", "Sonnet 5 (1M context)")];
    let filler = format!(
        r#"{{"type":"user","message":{{"content":"{}"}}}}"#,
        "x".repeat(4000)
    );
    for _ in 0..200 {
        lines.push(filler.clone());
    }
    lines.push(assistant("claude-sonnet-5"));
    let path = write_transcript("long_head", &lines);
    let size = std::fs::metadata(&path).expect("stat").len();
    assert!(
        size > 512 * 1024,
        "fixture must outgrow the tail read: {size}"
    );
    assert_eq!(detect_window(&path), Some(1_000_000));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn missing_transcript_returns_none() {
    assert_eq!(
        detect_window(&PathBuf::from("/nonexistent/squeez/transcript.jsonl")),
        None
    );
}
