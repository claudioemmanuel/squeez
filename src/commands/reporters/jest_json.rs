//! Structured condenser for `jest --json` / `vitest --reporter=json` output.
//!
//! Sniffs on the `numTotalTests` + `testResults` shape unique to jest's JSON
//! reporter; a non-match safely falls back to the generic byte-level filter.

use crate::json_util;

/// Max non-stack message lines kept per failing assertion (expected/actual etc).
const MAX_MESSAGE_LINES: usize = 6;
/// Max passing-suite file names named individually before collapsing to a count.
const MAX_PASSING_SUITES_NAMED: usize = 20;

pub fn sniff(text: &str) -> bool {
    text.contains("\"numTotalTests\"") && text.contains("\"testResults\"")
}

pub fn condense(lines: &[String]) -> Option<Vec<String>> {
    let joined = lines.join("\n");
    if !sniff(&joined) {
        return None;
    }
    let start = joined.find('{')?;
    let end = joined.rfind('}')?;
    if end <= start {
        return None;
    }
    let root = json_util::parse_value(&joined[start..=end])?;

    let num_total = root.get("numTotalTests").and_then(|v| v.as_u64()).unwrap_or(0);
    let num_failed = root.get("numFailedTests").and_then(|v| v.as_u64()).unwrap_or(0);
    let num_passed = root.get("numPassedTests").and_then(|v| v.as_u64()).unwrap_or(0);
    let num_pending = root.get("numPendingTests").and_then(|v| v.as_u64()).unwrap_or(0);
    let suites_failed = root
        .get("numFailedTestSuites")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let suites_total = root
        .get("numTotalTestSuites")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let mut out = Vec::new();
    if num_failed == 0 && suites_failed == 0 {
        out.push(format!(
            "ok {} passed ({} suite{})",
            num_passed,
            suites_total,
            if suites_total == 1 { "" } else { "s" }
        ));
        return Some(out);
    }

    out.push(format!(
        "FAIL {}/{} — {} suite{} failing",
        num_failed,
        num_total,
        suites_failed,
        if suites_failed == 1 { "" } else { "s" }
    ));

    let mut passed_suites: Vec<&str> = Vec::new();
    for tr in root.get("testResults").map(|v| v.as_arr()).unwrap_or(&[]) {
        if tr.get("status").and_then(|v| v.as_str()) == Some("passed") {
            if let Some(name) = tr.get("name").and_then(|v| v.as_str()) {
                passed_suites.push(name);
            }
            continue;
        }
        let assertions = tr.get("assertionResults").map(|v| v.as_arr()).unwrap_or(&[]);
        for a in assertions {
            if a.get("status").and_then(|v| v.as_str()) != Some("failed") {
                continue;
            }
            let name = a
                .get("fullName")
                .and_then(|v| v.as_str())
                .or_else(|| a.get("title").and_then(|v| v.as_str()))
                .unwrap_or("<unknown test>");
            out.push(format!("✗ {}", name));
            for fm in a.get("failureMessages").map(|v| v.as_arr()).unwrap_or(&[]).iter().take(1) {
                if let Some(msg) = fm.as_str() {
                    for l in condense_failure_message(msg) {
                        out.push(format!("  {}", l));
                    }
                }
            }
        }
        // Suite-level failure with no per-assertion detail (e.g. a syntax/import error).
        if assertions.is_empty() && tr.get("status").and_then(|v| v.as_str()) == Some("failed") {
            let file = tr.get("name").and_then(|v| v.as_str()).unwrap_or("<suite>");
            out.push(format!("✗ {} (suite error)", file));
            if let Some(msg) = tr.get("message").and_then(|v| v.as_str()) {
                for l in condense_failure_message(msg) {
                    out.push(format!("  {}", l));
                }
            }
        }
    }

    if !passed_suites.is_empty() {
        if passed_suites.len() <= MAX_PASSING_SUITES_NAMED {
            out.push(format!("passing suites: {}", passed_suites.join(", ")));
        } else {
            out.push(format!(
                "passing suites: {}, +{} more",
                passed_suites[..MAX_PASSING_SUITES_NAMED].join(", "),
                passed_suites.len() - MAX_PASSING_SUITES_NAMED
            ));
        }
    }
    out.push(format!(
        "{} passed, {} failed, {} pending",
        num_passed, num_failed, num_pending
    ));
    Some(out)
}

/// Keeps the non-stack message lines (assertion, expected/actual) capped at
/// `MAX_MESSAGE_LINES`, plus the first stack frame outside `node_modules`
/// (mirrors dropping vendored frames the way cargo's condenser drops std/core).
fn condense_failure_message(msg: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut local_frame = None;
    for line in msg.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with("at ") {
            if local_frame.is_none() && !t.contains("node_modules") {
                local_frame = Some(t.to_string());
            }
            continue;
        }
        if out.len() < MAX_MESSAGE_LINES {
            out.push(t.to_string());
        }
    }
    if let Some(frame) = local_frame {
        out.push(frame);
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
    fn non_jest_json_returns_none() {
        assert!(condense(&lines(" PASS  src/x.test.ts\n")).is_none());
        assert!(condense(&lines(r#"{"foo":"bar"}"#)).is_none());
    }

    #[test]
    fn all_passing_collapses_to_one_line() {
        let json = r#"{"numTotalTests":5,"numFailedTests":0,"numPassedTests":5,"numPendingTests":0,"numFailedTestSuites":0,"numTotalTestSuites":1,"success":true,"testResults":[{"name":"a.test.ts","status":"passed","message":"","assertionResults":[]}]}"#;
        let out = condense(&lines(json)).expect("should sniff jest json");
        assert_eq!(out.len(), 1);
        assert!(out[0].contains("ok 5 passed"));
    }

    #[test]
    fn failure_preserves_name_and_drops_node_modules_frames() {
        let json = r#"{"numTotalTests":3,"numFailedTests":1,"numPassedTests":2,"numPendingTests":0,"numFailedTestSuites":1,"numTotalTestSuites":1,"success":false,"testResults":[{"name":"/repo/src/api/client.test.ts","status":"failed","message":"","assertionResults":[{"fullName":"API client should retry on 503","title":"should retry on 503","status":"failed","failureMessages":["Error: expect(received).toHaveBeenCalledTimes(expected)\n\nExpected number of calls: 3\nReceived number of calls: 1\n    at Object.<anonymous> (/repo/src/api/client.test.ts:47:24)\n    at Promise.then.completed (/repo/node_modules/jest-circus/build/utils.js:298:28)"]},{"fullName":"API client should fetch","title":"should fetch","status":"passed","failureMessages":[]}]}]}"#;
        let out = condense(&lines(json)).expect("should sniff jest json");
        assert!(out.iter().any(|l| l.contains("FAIL 1/3")));
        assert!(out.iter().any(|l| l.contains("API client should retry on 503")));
        assert!(out.iter().any(|l| l.contains("Expected number of calls: 3")));
        assert!(out.iter().any(|l| l.contains("client.test.ts:47:24") && !l.contains("node_modules")));
        assert!(!out.iter().any(|l| l.contains("jest-circus")));
        assert!(!out.iter().any(|l| l.contains("should fetch")));
    }

    #[test]
    fn suite_level_failure_without_assertions() {
        let json = r#"{"numTotalTests":0,"numFailedTests":0,"numPassedTests":0,"numPendingTests":0,"numFailedTestSuites":1,"numTotalTestSuites":1,"success":false,"testResults":[{"name":"/repo/src/broken.test.ts","status":"failed","message":"SyntaxError: Unexpected token","assertionResults":[]}]}"#;
        let out = condense(&lines(json)).expect("should sniff jest json");
        assert!(out.iter().any(|l| l.contains("broken.test.ts")));
        assert!(out.iter().any(|l| l.contains("SyntaxError")));
    }
}
