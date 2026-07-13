//! Structured condenser for pytest's default text output.
//!
//! Sniffs on the final `==== N failed, M passed in Xs ====` summary line —
//! present in plain `pytest` stdout with no extra plugin or flag required.
//! (`--json-report` needs a plugin and writes to a *file*, not stdout, so it
//! isn't a reliable interception point for a bash-output compressor.)

const MAX_DETAIL_LINES: usize = 8;

pub fn sniff(lines: &[String]) -> bool {
    lines.iter().any(|l| is_summary_line(l))
}

fn is_summary_line(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('=')
        && t.ends_with('=')
        && (t.contains(" passed in ")
            || t.contains(" failed in ")
            || t.contains(" failed, ")
            || t.contains(" error in ")
            || t.contains(" errors in ")
            || t.contains("no tests ran in"))
}

#[derive(Default)]
struct Counts {
    passed: u64,
    failed: u64,
    skipped: u64,
    errors: u64,
    xfailed: u64,
    xpassed: u64,
    time: String,
}

fn first_u64(s: &str) -> u64 {
    s.split_whitespace()
        .find_map(|w| w.parse::<u64>().ok())
        .unwrap_or(0)
}

fn parse_summary(line: &str) -> Counts {
    let t = line.trim().trim_matches('=').trim();
    let (body, time) = match t.rfind(" in ") {
        Some(idx) => (&t[..idx], t[idx + 4..].trim()),
        None => (t, ""),
    };
    let mut c = Counts {
        time: time.to_string(),
        ..Default::default()
    };
    for part in body.split(',') {
        let p = part.trim();
        let n = first_u64(p);
        // Check xfailed/xpassed before failed/passed — the former contain
        // the latter as substrings ("xfailed".contains("failed") == true).
        if p.contains("xfailed") {
            c.xfailed = n;
        } else if p.contains("xpassed") {
            c.xpassed = n;
        } else if p.contains("failed") {
            c.failed = n;
        } else if p.contains("passed") {
            c.passed = n;
        } else if p.contains("skipped") {
            c.skipped = n;
        } else if p.contains("error") {
            c.errors = n;
        }
    }
    c
}

pub fn condense(lines: &[String]) -> Option<Vec<String>> {
    let summary_idx = lines.iter().rposition(|l| is_summary_line(l))?;
    let counts = parse_summary(&lines[summary_idx]);
    let total = counts.passed + counts.failed + counts.skipped + counts.errors + counts.xfailed + counts.xpassed;

    let mut out = Vec::new();
    if counts.failed == 0 && counts.errors == 0 {
        let mut line = format!("ok {} passed", counts.passed);
        if counts.skipped > 0 {
            line.push_str(&format!(", {} skipped", counts.skipped));
        }
        if !counts.time.is_empty() {
            line.push_str(&format!(" ({})", counts.time));
        }
        out.push(line);
        return Some(out);
    }

    out.push(format!("FAIL {}/{}", counts.failed + counts.errors, total));

    let failed_files = if let Some(names) = collect_from_short_summary(lines, summary_idx) {
        let files = failing_files(&names);
        out.extend(names);
        files
    } else {
        let names = collect_from_failures_section(lines, summary_idx);
        let files = failing_files(&names);
        out.extend(names);
        files
    };

    if let Some(passing) = passing_files(lines, summary_idx, &failed_files) {
        out.push(passing);
    }

    out.push(format!(
        "{} passed, {} failed, {} errors, {} skipped",
        counts.passed, counts.failed, counts.errors, counts.skipped
    ));
    Some(out)
}

const MAX_PASSING_FILES_NAMED: usize = 20;

fn failing_files(failure_lines: &[String]) -> std::collections::HashSet<String> {
    failure_lines
        .iter()
        .filter_map(|l| l.strip_prefix("✗ "))
        .filter_map(|l| l.split("::").next())
        .map(|s| s.trim().to_string())
        .collect()
}

/// Verbose pytest runs (`-v`) print one `path::test PASSED` line per test —
/// collect the distinct file paths so a fully-clean file isn't silently
/// dropped from the condensed output (economy::preservation penalizes
/// dropping every mention of a file path).
fn passing_files(
    lines: &[String],
    summary_idx: usize,
    failed_files: &std::collections::HashSet<String>,
) -> Option<String> {
    let mut seen = std::collections::HashSet::new();
    let mut ordered = Vec::new();
    for l in &lines[..summary_idx] {
        let t = l.trim();
        if let Some(rest) = t.strip_suffix(" PASSED") {
            if let Some(file) = rest.split("::").next() {
                let file = file.trim().to_string();
                if !failed_files.contains(&file) && seen.insert(file.clone()) {
                    ordered.push(file);
                }
            }
        }
    }
    if ordered.is_empty() {
        return None;
    }
    if ordered.len() <= MAX_PASSING_FILES_NAMED {
        Some(format!("passing files: {}", ordered.join(", ")))
    } else {
        Some(format!(
            "passing files: {}, +{} more",
            ordered[..MAX_PASSING_FILES_NAMED].join(", "),
            ordered.len() - MAX_PASSING_FILES_NAMED
        ))
    }
}

/// Preferred source: pytest's own "short test summary info" section, which is
/// already a compact `FAILED path::test - Reason` / `ERROR path::test - Reason`
/// list — just re-tag it, no reparsing needed.
fn collect_from_short_summary(lines: &[String], summary_idx: usize) -> Option<Vec<String>> {
    let sts_idx = lines[..summary_idx]
        .iter()
        .rposition(|l| l.trim().trim_matches('=').trim() == "short test summary info")?;
    let locations = failure_locations(lines, sts_idx);
    let mut out = Vec::new();
    for l in &lines[sts_idx + 1..summary_idx] {
        let t = l.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(rest) = t.strip_prefix("FAILED ").or_else(|| t.strip_prefix("ERROR ")) {
            out.push(format!("✗ {}", rest));
            let short_name = rest
                .split(" - ")
                .next()
                .unwrap_or(rest)
                .rsplit("::")
                .next()
                .unwrap_or(rest);
            if let Some(loc) = locations.get(short_name) {
                out.push(format!("  {}", loc));
            }
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Maps bare test name -> its `path.py:LINE: ExceptionType` location line,
/// sourced from the `FAILURES` section (the short-summary list alone doesn't
/// carry line numbers).
fn failure_locations(lines: &[String], end: usize) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let mut i = 0;
    while i < end {
        let t = lines[i].trim();
        if t.len() > 4 && t.starts_with('_') && t.ends_with('_') {
            let name = t.trim_matches('_').trim().to_string();
            let mut j = i + 1;
            while j < end {
                let lt = lines[j].trim();
                if lt.starts_with('_') && lt.ends_with('_') && lt.len() > 4 {
                    break;
                }
                if lt.contains(".py:") && !name.is_empty() {
                    map.insert(name.clone(), lt.to_string());
                }
                j += 1;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    map
}

/// Fallback when no short-summary section is present: scan the `FAILURES`
/// section for `___ test_name ___` headers and the `E   ...` assertion lines
/// that follow each one.
fn collect_from_failures_section(lines: &[String], summary_idx: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < summary_idx {
        let t = lines[i].trim();
        if t.len() > 4 && t.starts_with('_') && t.ends_with('_') {
            let name = t.trim_matches('_').trim();
            if !name.is_empty() {
                out.push(format!("✗ {}", name));
                let mut j = i + 1;
                let mut detail = 0;
                let mut last_location: Option<String> = None;
                while j < summary_idx {
                    let lt = lines[j].trim();
                    if lt.starts_with('_') && lt.ends_with('_') && lt.len() > 4 {
                        break;
                    }
                    if let Some(msg) = lt.strip_prefix("E ").or_else(|| lt.strip_prefix("E\t")) {
                        if detail < MAX_DETAIL_LINES {
                            out.push(format!("  {}", msg.trim()));
                            detail += 1;
                        }
                    } else if lt.contains(".py:") {
                        last_location = Some(lt.to_string());
                    }
                    j += 1;
                }
                if let Some(loc) = last_location {
                    out.push(format!("  {}", loc));
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(|l| l.to_string()).collect()
    }

    #[test]
    fn non_pytest_output_returns_none() {
        assert!(condense(&lines("running 3 tests\ntest a ... ok\n")).is_none());
    }

    #[test]
    fn all_passing_collapses_to_one_line() {
        let input = lines(
            "============================= test session starts ==============================\n\
collected 5 items\n\
\n\
tests/test_a.py .....                                                          [100%]\n\
\n\
============================== 5 passed in 0.12s ===============================\n",
        );
        let out = condense(&input).expect("should sniff pytest output");
        assert_eq!(out.len(), 1);
        assert!(out[0].contains("ok 5 passed"));
        assert!(out[0].contains("0.12s"));
    }

    #[test]
    fn failures_preserved_via_short_summary() {
        let input = lines(
            "============================= test session starts ==============================\n\
collected 312 items\n\
\n\
tests/test_auth.py::test_token_expiry FAILED\n\
\n\
================================= FAILURES =================================\n\
___________________________ test_token_expiry ___________________________\n\
\n\
    def test_token_expiry():\n\
>       assert verify_token(expired_token) is False\n\
E       AssertionError: assert True is False\n\
\n\
tests/test_auth.py:88: AssertionError\n\
=========================== short test summary info ============================\n\
FAILED tests/test_auth.py::test_token_expiry - AssertionError\n\
========================= 1 failed, 280 passed in 14.32s =========================\n",
        );
        let out = condense(&input).expect("should sniff pytest output");
        assert!(out.iter().any(|l| l.contains("FAIL 1/281")));
        assert!(out.iter().any(|l| l.contains("tests/test_auth.py::test_token_expiry") && l.contains("AssertionError")));
        assert!(out.iter().any(|l| l == "280 passed, 1 failed, 0 errors, 0 skipped"));
    }

    #[test]
    fn fallback_to_failures_section_without_short_summary() {
        let input = lines(
            "collected 2 items\n\
\n\
================================= FAILURES =================================\n\
___________________________ test_thing ___________________________\n\
\n\
>       assert 1 == 2\n\
E       assert 1 == 2\n\
\n\
tests/test_x.py:10: AssertionError\n\
========================= 1 failed, 1 passed in 0.01s =========================\n",
        );
        let out = condense(&input).expect("should sniff pytest output");
        assert!(out.iter().any(|l| l.contains("test_thing")));
        assert!(out.iter().any(|l| l.contains("assert 1 == 2")));
        assert!(out.iter().any(|l| l.contains("test_x.py:10")));
    }

    #[test]
    fn xfail_not_miscounted_as_failed() {
        let input = lines(
            "========================= 2 xfailed, 3 passed in 0.05s =========================\n",
        );
        let out = condense(&input).expect("should sniff pytest output");
        assert_eq!(out.len(), 1);
        assert!(out[0].contains("ok 3 passed"));
    }
}
