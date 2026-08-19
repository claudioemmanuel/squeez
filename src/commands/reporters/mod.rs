//! Structured reporters: parse a known tool's machine-readable output shape
//! into a dense summary (test runners into failures-only; `git status`
//! porcelain into a working-tree summary). Each reporter sniffs its own format
//! and returns `None` when the shape doesn't match, so callers can fall back
//! to the generic byte-level filter without risk of misparsing.

pub mod cargo_test;
pub mod eslint_json;
pub mod git_status;
pub mod go_test_ndjson;
pub mod jest_json;
pub mod pytest;
pub mod ruff_json;

/// Try each structured reporter in turn; `None` means no reporter recognized
/// the output and the caller should fall back to generic filtering.
pub fn detect_and_condense(cmd: &str, lines: &[String]) -> Option<Vec<String>> {
    git_status::condense(cmd, lines)
        .or_else(|| cargo_test::condense(lines))
        .or_else(|| jest_json::condense(lines))
        .or_else(|| pytest::condense(lines))
        .or_else(|| go_test_ndjson::condense(lines))
        .or_else(|| eslint_json::condense(cmd, lines))
        .or_else(|| ruff_json::condense(cmd, lines))
}
