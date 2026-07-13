//! Structured test-reporter condensers: parse a known test-runner's output
//! shape into a failures-only summary. Each reporter sniffs its own format
//! and returns `None` when the shape doesn't match, so callers can fall back
//! to the generic byte-level filter without risk of misparsing.

pub mod cargo_test;
pub mod go_test_ndjson;
pub mod jest_json;
pub mod pytest;

/// Try each structured reporter in turn; `None` means no reporter recognized
/// the output and the caller should fall back to generic filtering.
pub fn detect_and_condense(_cmd: &str, lines: &[String]) -> Option<Vec<String>> {
    cargo_test::condense(lines)
        .or_else(|| jest_json::condense(lines))
        .or_else(|| pytest::condense(lines))
        .or_else(|| go_test_ndjson::condense(lines))
}
