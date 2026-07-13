//! Structured condenser for `cargo test` / `rustc`'s built-in test harness output.
//!
//! Sniffs on `test result:` (unique to the built-in harness — nextest and
//! other runners don't emit it), so a non-match safely falls back to the
//! generic byte-level filter in `test_runner.rs`.

/// Max detail lines kept per failing test (panic location + assertion/message).
const MAX_DETAIL_LINES: usize = 8;

pub fn sniff(lines: &[String]) -> bool {
    lines.iter().any(|l| l.trim_start().starts_with("test result:"))
}

pub fn condense(lines: &[String]) -> Option<Vec<String>> {
    if !sniff(lines) {
        return None;
    }

    let detail_blocks = collect_detail_blocks(lines);

    let mut total_passed = 0u64;
    let mut total_failed = 0u64;
    let mut total_ignored = 0u64;
    let mut suites_total = 0u64;
    let mut suites_failed = 0u64;
    let mut total_time = 0f64;
    let mut current_names: Vec<String> = Vec::new();
    let mut ordered_failures: Vec<String> = Vec::new();

    let mut idx = 0;
    while idx < lines.len() {
        let trimmed = lines[idx].trim();
        if trimmed == "failures:" {
            // The header "failures:" (before per-test panic dumps) is followed
            // by a blank line; the footer "failures:" (the exact name list) is
            // followed immediately by indented test names.
            if idx + 1 < lines.len() && !lines[idx + 1].trim().is_empty() {
                let mut m = idx + 1;
                current_names.clear();
                while m < lines.len() && !lines[m].trim().is_empty() {
                    current_names.push(lines[m].trim().to_string());
                    m += 1;
                }
                idx = m;
                continue;
            }
        } else if let Some(rest) = trimmed.strip_prefix("test result:") {
            suites_total += 1;
            let (passed, failed, ignored, time) = parse_result_counts(rest);
            total_passed += passed;
            total_failed += failed;
            total_ignored += ignored;
            if let Some(t) = time.trim_end_matches('s').parse::<f64>().ok() {
                total_time += t;
            }
            if failed > 0 {
                suites_failed += 1;
                ordered_failures.extend(current_names.drain(..));
            } else {
                current_names.clear();
            }
        }
        idx += 1;
    }

    let mut out = Vec::new();
    if total_failed == 0 {
        out.push(format!(
            "ok {} passed ({} suite{}, {:.2}s)",
            total_passed,
            suites_total,
            if suites_total == 1 { "" } else { "s" },
            total_time
        ));
        return Some(out);
    }

    out.push(format!(
        "FAIL {}/{} — {} suite{} failing",
        total_failed,
        total_passed + total_failed + total_ignored,
        suites_failed,
        if suites_failed == 1 { "" } else { "s" }
    ));
    let mut seen = std::collections::HashSet::new();
    for name in &ordered_failures {
        if !seen.insert(name.clone()) {
            continue;
        }
        out.push(format!("✗ {}", name));
        if let Some(block) = detail_blocks.get(name) {
            for l in block {
                out.push(format!("  {}", l.trim_end()));
            }
        }
    }
    out.push(format!(
        "{} passed, {} failed, {} ignored",
        total_passed, total_failed, total_ignored
    ));
    Some(out)
}

/// Map failing-test-name -> capped panic/assertion detail lines, sourced from
/// `---- <name> stdout ----` / `---- <name> stderr ----` blocks.
fn collect_detail_blocks(lines: &[String]) -> std::collections::HashMap<String, Vec<String>> {
    let mut blocks = std::collections::HashMap::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if let Some(rest) = trimmed.strip_prefix("---- ") {
            let name = rest
                .strip_suffix(" stdout ----")
                .or_else(|| rest.strip_suffix(" stderr ----"));
            if let Some(name) = name {
                let name = name.to_string();
                let mut block = Vec::new();
                let mut j = i + 1;
                while j < lines.len() {
                    let lt = lines[j].trim();
                    if lt.starts_with("---- ") || lt == "failures:" {
                        break;
                    }
                    if lt.is_empty() {
                        // A blank line right after a non-empty block ends it;
                        // a leading blank (rare) is just skipped.
                        if !block.is_empty() {
                            break;
                        }
                        j += 1;
                        continue;
                    }
                    if lt.starts_with("note: run with `RUST_BACKTRACE") {
                        j += 1;
                        continue;
                    }
                    if block.len() < MAX_DETAIL_LINES {
                        block.push(lines[j].clone());
                    }
                    j += 1;
                }
                blocks.entry(name).or_insert(block);
                i = j;
                continue;
            }
        }
        i += 1;
    }
    blocks
}

fn first_u64(s: &str) -> u64 {
    s.split_whitespace()
        .find_map(|w| w.trim_end_matches('.').parse::<u64>().ok())
        .unwrap_or(0)
}

/// Parses the tail of a `test result:` line, e.g.
/// ` FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s`
fn parse_result_counts(rest: &str) -> (u64, u64, u64, String) {
    let mut passed = 0;
    let mut failed = 0;
    let mut ignored = 0;
    let mut time = String::new();
    for part in rest.split(';') {
        let p = part.trim();
        if let Some(t) = p.strip_prefix("finished in ") {
            time = t.trim().to_string();
        } else if p.contains("passed") {
            passed = first_u64(p);
        } else if p.contains("failed") {
            failed = first_u64(p);
        } else if p.contains("ignored") {
            ignored = first_u64(p);
        }
    }
    (passed, failed, ignored, time)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(|l| l.to_string()).collect()
    }

    #[test]
    fn sniff_requires_test_result_line() {
        assert!(!sniff(&lines("some random output\nno match here")));
        assert!(sniff(&lines(
            "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s"
        )));
    }

    #[test]
    fn all_passing_collapses_to_one_line() {
        let input = lines(
            "running 3 tests\n\
test tests::a ... ok\n\
test tests::b ... ok\n\
test tests::c ... ok\n\
\n\
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s\n",
        );
        let out = condense(&input).expect("should sniff cargo test format");
        assert_eq!(out.len(), 1);
        assert!(out[0].contains("ok 3 passed"));
    }

    #[test]
    fn failure_preserves_name_and_panic_detail() {
        let input = lines(
            "running 3 tests\n\
test tests::bar ... FAILED\n\
test tests::baz ... ok\n\
test tests::foo ... ok\n\
\n\
failures:\n\
\n\
---- tests::bar stdout ----\n\
thread 'tests::bar' panicked at src/lib.rs:42:5:\n\
assertion `left == right` failed\n\
  left: 1\n\
  right: 2\n\
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n\
\n\
\n\
failures:\n\
    tests::bar\n\
\n\
test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
        );
        let out = condense(&input).expect("should sniff cargo test format");
        assert!(out.iter().any(|l| l.contains("FAIL 1/3")));
        assert!(out.iter().any(|l| l.contains("tests::bar")));
        assert!(out.iter().any(|l| l.contains("src/lib.rs:42:5")));
        assert!(out.iter().any(|l| l.contains("left == right")));
        // Boilerplate backtrace hint must be dropped.
        assert!(!out.iter().any(|l| l.contains("RUST_BACKTRACE")));
        // Passing tests never appear.
        assert!(!out.iter().any(|l| l.contains("tests::baz")));
    }

    #[test]
    fn multiple_binaries_aggregate_totals() {
        let input = lines(
            "running 2 tests\n\
test a ... ok\n\
test b ... ok\n\
\n\
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s\n\
\n\
running 2 tests\n\
test c ... FAILED\n\
test d ... ok\n\
\n\
failures:\n\
\n\
---- c stdout ----\n\
thread 'c' panicked at tests/it.rs:5:1:\n\
boom\n\
\n\
\n\
failures:\n\
    c\n\
\n\
test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
        );
        let out = condense(&input).expect("should sniff cargo test format");
        assert!(out.iter().any(|l| l.contains("FAIL 1/4")));
        assert!(out.iter().any(|l| l.contains("1 suite failing")));
        assert!(out.iter().any(|l| l == "3 passed, 1 failed, 0 ignored"));
    }

    #[test]
    fn non_cargo_output_returns_none() {
        let input = lines(" PASS  src/x.test.ts\nTest Suites: 1 passed, 1 total\n");
        assert!(condense(&input).is_none());
    }
}
