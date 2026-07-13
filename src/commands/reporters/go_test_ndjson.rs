//! Structured condenser for `go test -json` output — one JSON object per
//! line (`{"Action":"pass","Package":"...","Test":"...","Output":"..."}`).
//!
//! Sniffs on at least two lines shaped like a go test event; a non-match
//! falls back to the generic byte-level filter.

use crate::json_util;
use std::collections::{HashMap, HashSet};

const MAX_DETAIL_LINES: usize = 8;
const MAX_PASSING_PKGS_NAMED: usize = 20;

pub fn sniff(lines: &[String]) -> bool {
    lines
        .iter()
        .filter(|l| {
            let t = l.trim();
            t.starts_with('{') && t.ends_with('}') && t.contains("\"Action\"") && t.contains("\"Package\"")
        })
        .count()
        >= 2
}

#[derive(Clone, PartialEq)]
enum Outcome {
    Pass,
    Fail,
    Skip,
}

pub fn condense(lines: &[String]) -> Option<Vec<String>> {
    if !sniff(lines) {
        return None;
    }

    let mut outputs: HashMap<(String, String), Vec<String>> = HashMap::new();
    let mut outcomes: HashMap<(String, String), Outcome> = HashMap::new();
    let mut order: Vec<(String, String)> = Vec::new();
    let mut pkg_build_failed: Vec<String> = Vec::new();
    let mut all_packages: Vec<String> = Vec::new();
    let mut seen_pkgs: HashSet<String> = HashSet::new();
    let (mut total_pass, mut total_fail, mut total_skip) = (0u64, 0u64, 0u64);

    for line in lines {
        let t = line.trim();
        if t.is_empty() || !t.starts_with('{') {
            continue;
        }
        let v = match json_util::parse_value(t) {
            Some(v) => v,
            None => continue,
        };
        let action = v.get("Action").and_then(|x| x.as_str()).unwrap_or("");
        let package = v.get("Package").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let test = v.get("Test").and_then(|x| x.as_str()).map(|s| s.to_string());

        if !package.is_empty() && seen_pkgs.insert(package.clone()) {
            all_packages.push(package.clone());
        }

        match (action, &test) {
            ("output", _) => {
                if let Some(out) = v.get("Output").and_then(|x| x.as_str()) {
                    let key = (package.clone(), test.clone().unwrap_or_default());
                    outputs.entry(key).or_default().push(out.to_string());
                }
            }
            ("pass", Some(tn)) => {
                let key = (package.clone(), tn.clone());
                if !outcomes.contains_key(&key) {
                    order.push(key.clone());
                }
                outcomes.insert(key, Outcome::Pass);
                total_pass += 1;
            }
            ("fail", Some(tn)) => {
                let key = (package.clone(), tn.clone());
                if !outcomes.contains_key(&key) {
                    order.push(key.clone());
                }
                outcomes.insert(key, Outcome::Fail);
                total_fail += 1;
            }
            ("skip", Some(tn)) => {
                let key = (package.clone(), tn.clone());
                if !outcomes.contains_key(&key) {
                    order.push(key.clone());
                }
                outcomes.insert(key, Outcome::Skip);
                total_skip += 1;
            }
            ("fail", None) => {
                // Package-level failure with no per-test events at all
                // (build/compile error) shows up only here.
                if !order.iter().any(|(p, _)| p == &package) {
                    pkg_build_failed.push(package.clone());
                }
            }
            _ => {}
        }
    }

    let failing_pkgs: HashSet<&str> = order
        .iter()
        .filter(|k| outcomes.get(*k) == Some(&Outcome::Fail))
        .map(|(p, _)| p.as_str())
        .collect();
    let build_failed_count = pkg_build_failed.len() as u64;
    let total = total_pass + total_fail + total_skip;

    let mut out = Vec::new();
    if total_fail == 0 && pkg_build_failed.is_empty() {
        let mut line = format!("ok {} passed", total_pass);
        if total_skip > 0 {
            line.push_str(&format!(", {} skipped", total_skip));
        }
        out.push(line);
        return Some(out);
    }

    out.push(format!(
        "FAIL {}/{} — {} package{} affected",
        total_fail + build_failed_count,
        total + build_failed_count,
        failing_pkgs.len() + pkg_build_failed.len(),
        if failing_pkgs.len() + pkg_build_failed.len() == 1 { "" } else { "s" }
    ));

    for key in &order {
        if outcomes.get(key) != Some(&Outcome::Fail) {
            continue;
        }
        let (pkg, test) = key;
        out.push(format!("✗ {}::{}", pkg, test));
        if let Some(raw) = outputs.get(key) {
            for l in condense_go_output(raw) {
                out.push(format!("  {}", l));
            }
        }
    }
    for pkg in &pkg_build_failed {
        out.push(format!("✗ {} (build failed)", pkg));
        if let Some(raw) = outputs.get(&(pkg.clone(), String::new())) {
            for l in condense_go_output(raw) {
                out.push(format!("  {}", l));
            }
        }
    }

    let passing_pkgs: Vec<&String> = all_packages
        .iter()
        .filter(|p| !failing_pkgs.contains(p.as_str()) && !pkg_build_failed.contains(p))
        .collect();
    if !passing_pkgs.is_empty() {
        if passing_pkgs.len() <= MAX_PASSING_PKGS_NAMED {
            out.push(format!(
                "passing packages: {}",
                passing_pkgs.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            ));
        } else {
            out.push(format!(
                "passing packages: {}, +{} more",
                passing_pkgs[..MAX_PASSING_PKGS_NAMED]
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                passing_pkgs.len() - MAX_PASSING_PKGS_NAMED
            ));
        }
    }

    out.push(format!(
        "{} passed, {} failed, {} skipped",
        total_pass, total_fail, total_skip
    ));
    Some(out)
}

/// Drops the redundant `--- FAIL: Name (Xs)` / bare `FAIL` / `PASS` framing
/// lines (already conveyed by our own header) and caps the rest.
fn condense_go_output(raw: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for chunk in raw {
        for line in chunk.lines() {
            let t = line.trim();
            if t.is_empty() || t == "FAIL" || t == "PASS" || t.starts_with("--- FAIL") || t.starts_with("--- PASS") {
                continue;
            }
            if out.len() < MAX_DETAIL_LINES {
                out.push(t.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn non_ndjson_returns_none() {
        assert!(condense(&lines(&["running 3 tests", "test a ... ok"])).is_none());
    }

    #[test]
    fn all_passing_collapses_to_one_line() {
        let input = lines(&[
            r#"{"Action":"run","Package":"pkg/a","Test":"TestFoo"}"#,
            r#"{"Action":"pass","Package":"pkg/a","Test":"TestFoo","Elapsed":0.01}"#,
            r#"{"Action":"pass","Package":"pkg/a","Elapsed":0.02}"#,
        ]);
        let out = condense(&input).expect("should sniff go test ndjson");
        assert_eq!(out.len(), 1);
        assert!(out[0].contains("ok 1 passed"));
    }

    #[test]
    fn failure_preserves_test_name_and_output() {
        let input = lines(&[
            r#"{"Action":"run","Package":"pkg/a","Test":"TestFoo"}"#,
            "{\"Action\":\"output\",\"Package\":\"pkg/a\",\"Test\":\"TestFoo\",\"Output\":\"--- FAIL: TestFoo (0.00s)\\n\"}",
            "{\"Action\":\"output\",\"Package\":\"pkg/a\",\"Test\":\"TestFoo\",\"Output\":\"    foo_test.go:15: expected 2, got 3\\n\"}",
            r#"{"Action":"fail","Package":"pkg/a","Test":"TestFoo","Elapsed":0.00}"#,
            r#"{"Action":"run","Package":"pkg/a","Test":"TestBar"}"#,
            r#"{"Action":"pass","Package":"pkg/a","Test":"TestBar","Elapsed":0.00}"#,
            r#"{"Action":"fail","Package":"pkg/a","Elapsed":0.01}"#,
            r#"{"Action":"run","Package":"pkg/b","Test":"TestBaz"}"#,
            r#"{"Action":"pass","Package":"pkg/b","Test":"TestBaz","Elapsed":0.00}"#,
            r#"{"Action":"pass","Package":"pkg/b","Elapsed":0.00}"#,
        ]);
        let out = condense(&input).expect("should sniff go test ndjson");
        assert!(out.iter().any(|l| l.contains("FAIL 1/3")));
        assert!(out.iter().any(|l| l.contains("pkg/a::TestFoo")));
        assert!(out.iter().any(|l| l.contains("foo_test.go:15: expected 2, got 3")));
        assert!(!out.iter().any(|l| l.contains("TestBar") && l.starts_with("✗")));
        assert!(out.iter().any(|l| l.contains("passing packages: pkg/b")));
        assert!(!out.iter().any(|l| l.contains("--- FAIL")));
    }

    #[test]
    fn build_failure_with_no_test_events() {
        let input = lines(&[
            "{\"Action\":\"output\",\"Package\":\"pkg/broken\",\"Output\":\"# pkg/broken\\n\"}",
            "{\"Action\":\"output\",\"Package\":\"pkg/broken\",\"Output\":\"./broken.go:3:2: undefined: Foo\\n\"}",
            r#"{"Action":"fail","Package":"pkg/broken","Elapsed":0.01}"#,
            r#"{"Action":"run","Package":"pkg/ok","Test":"TestOk"}"#,
            r#"{"Action":"pass","Package":"pkg/ok","Test":"TestOk","Elapsed":0.00}"#,
        ]);
        let out = condense(&input).expect("should sniff go test ndjson");
        assert!(out.iter().any(|l| l.contains("pkg/broken (build failed)")));
        assert!(out.iter().any(|l| l.contains("undefined: Foo")));
    }
}
