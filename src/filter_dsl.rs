//! User-declarable filter pipelines (E8) — a small, zero-dep DSL for the
//! long tail of commands `GenericHandler` can't meaningfully compress.
//! Subset-INI, extending `config.rs`'s line parser with `[filter "name"]`
//! sections holding an ORDERED pipeline of stages (order in the file is
//! preserved — this is not a flat key=value map).
//!
//! Layering (first match wins): `./.squeez/filters.ini` (project) >
//! `~/.claude/squeez/filters.ini` (user) > none (falls back to the normal
//! handler dispatch in `filter.rs`).
//!
//! Matching a command to a filter section: the section name is treated as
//! a prefix of the command string (`"cargo tree"` matches `cargo tree
//! --depth 2`); the longest matching section name wins so a more specific
//! rule beats a shorter one.
//!
//! Patterns (match_line/keep_lines_containing/strip_lines_containing) are
//! literal substrings, or a simple `*`-glob — never a regex crate.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum Stage {
    StripAnsi,
    Replace { from: String, to: String },
    /// Keep only lines matching `pattern`, unless they also match `unless`
    /// (the anti-error-swallowing guard: a line that would be dropped for
    /// matching `pattern` survives anyway if it also looks like an error).
    MatchLine { pattern: String, unless: Option<String> },
    KeepLinesContaining(String),
    StripLinesContaining(String),
    /// Truncates each line at the first occurrence of `marker` (marker and
    /// everything after it is dropped). No-op on lines without a match.
    TruncateLineAt(String),
    Head(usize),
    Tail(usize),
    MaxLines(usize),
    /// Text to emit instead of nothing when the pipeline empties the input.
    OnEmpty(String),
}

#[derive(Debug, Clone, Default)]
pub struct FilterDef {
    pub name: String,
    pub stages: Vec<Stage>,
    /// Inline self-tests: `test_input = "..."` / `test_expect = "..."`
    /// pairs, in file order, checked by `squeez filter-test`.
    pub tests: Vec<(String, String)>,
}

/// Parses a filters.ini file's content into its `[filter "name"]` sections.
/// Unknown/malformed directive lines are skipped rather than erroring —
/// same tolerance as `config.rs`'s parser, so a typo doesn't crash a wrap
/// call. Stage order within a section is the order in the file.
pub fn parse(content: &str) -> Vec<FilterDef> {
    let mut defs: Vec<FilterDef> = Vec::new();
    let mut pending_test_input: Option<String> = None;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = parse_section_header(line) {
            defs.push(FilterDef { name, stages: Vec::new(), tests: Vec::new() });
            pending_test_input = None;
            continue;
        }
        let Some(current) = defs.last_mut() else {
            continue; // directive before any [filter "..."] header — ignore
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        // NOTE: `value` is deliberately NOT unquoted here -- stages like
        // "replace" and "match_line" carry TWO quoted sub-values
        // (`"from" -> "to"`, `"pattern" unless "guard"`); unquoting the
        // whole line first would strip the outer quotes of the *pair* and
        // corrupt both sides. Single-value stages unquote their own value.
        let value = value.trim();

        match key {
            "strip_ansi" => {
                if value == "true" {
                    current.stages.push(Stage::StripAnsi);
                }
            }
            "replace" => {
                if let Some((from, to)) = parse_replace(value) {
                    current.stages.push(Stage::Replace { from, to });
                }
            }
            "match_line" => {
                let (pattern, unless) = parse_match_line(value);
                current.stages.push(Stage::MatchLine { pattern, unless });
            }
            "keep_lines_containing" => {
                current.stages.push(Stage::KeepLinesContaining(unquote(value)))
            }
            "strip_lines_containing" => {
                current.stages.push(Stage::StripLinesContaining(unquote(value)))
            }
            "truncate_line_at" => current.stages.push(Stage::TruncateLineAt(unquote(value))),
            "head" => {
                if let Ok(n) = value.parse() {
                    current.stages.push(Stage::Head(n));
                }
            }
            "tail" => {
                if let Ok(n) = value.parse() {
                    current.stages.push(Stage::Tail(n));
                }
            }
            "max_lines" => {
                if let Ok(n) = value.parse() {
                    current.stages.push(Stage::MaxLines(n));
                }
            }
            "on_empty" => current.stages.push(Stage::OnEmpty(unquote(value))),
            "test_input" => pending_test_input = Some(unquote(value)),
            "test_expect" => {
                if let Some(input) = pending_test_input.take() {
                    current.tests.push((input, unquote(value)));
                }
            }
            _ => {}
        }
    }
    defs
}

fn parse_section_header(line: &str) -> Option<String> {
    // [filter "name"]
    let inner = line.strip_prefix("[filter")?.trim();
    let inner = inner.strip_suffix(']')?.trim();
    let inner = inner.strip_prefix('"')?;
    let inner = inner.strip_suffix('"')?;
    Some(inner.to_string())
}

/// `"from" -> "to"` — either side may be empty (`"" -> ""` is a no-op but
/// harmless; an empty `from` never matches anything so it's just dropped
/// as unusable rather than special-cased).
fn parse_replace(value: &str) -> Option<(String, String)> {
    let (from, to) = value.split_once("->")?;
    let from = unquote(from.trim());
    let to = unquote(to.trim());
    if from.is_empty() {
        return None;
    }
    Some((from, to))
}

/// `"pattern"` or `"pattern" unless "guard"`.
fn parse_match_line(value: &str) -> (String, Option<String>) {
    match value.split_once(" unless ") {
        Some((pat, guard)) => (unquote(pat.trim()), Some(unquote(guard.trim()))),
        None => (unquote(value), None),
    }
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Literal substring match, or a simple `*`-glob (never a regex). No `*` in
/// `pattern` means plain substring containment; `*` splits the pattern
/// into ordered segments that must appear in order, anchored to the start
/// unless the pattern starts with `*`, and to the end unless it ends with
/// `*`.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    if !pattern.contains('*') {
        return text.contains(pattern);
    }
    let anchor_start = !pattern.starts_with('*');
    let anchor_end = !pattern.ends_with('*');
    let parts: Vec<&str> = pattern.split('*').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return true; // pattern was just "*" (or "**" etc.)
    }
    let mut pos = 0usize;
    for (i, part) in parts.iter().enumerate() {
        let is_first = i == 0;
        let is_last = i == parts.len() - 1;
        if is_first && anchor_start {
            if !text[pos..].starts_with(part) {
                return false;
            }
            pos += part.len();
        } else if is_last && anchor_end {
            if !text[pos..].ends_with(part) {
                return false;
            }
        } else {
            match text[pos..].find(part) {
                Some(idx) => pos += idx + part.len(),
                None => return false,
            }
        }
    }
    true
}

/// Runs a filter's ordered pipeline over `lines`.
pub fn apply(def: &FilterDef, lines: Vec<String>) -> Vec<String> {
    let mut lines = lines;
    for stage in &def.stages {
        lines = apply_stage(stage, lines);
    }
    lines
}

fn apply_stage(stage: &Stage, lines: Vec<String>) -> Vec<String> {
    match stage {
        Stage::StripAnsi => lines.into_iter().map(|l| strip_ansi(&l)).collect(),
        Stage::Replace { from, to } => {
            lines.into_iter().map(|l| l.replace(from.as_str(), to.as_str())).collect()
        }
        Stage::MatchLine { pattern, unless } => lines
            .into_iter()
            .filter(|l| {
                let matches = glob_match(pattern, l);
                let guarded = unless.as_ref().is_some_and(|u| glob_match(u, l));
                matches || guarded
            })
            .collect(),
        Stage::KeepLinesContaining(pattern) => {
            lines.into_iter().filter(|l| glob_match(pattern, l)).collect()
        }
        Stage::StripLinesContaining(pattern) => {
            lines.into_iter().filter(|l| !glob_match(pattern, l)).collect()
        }
        Stage::TruncateLineAt(marker) => lines
            .into_iter()
            .map(|l| match l.find(marker.as_str()) {
                Some(idx) => l[..idx].to_string(),
                None => l,
            })
            .collect(),
        Stage::Head(n) => lines.into_iter().take(*n).collect(),
        Stage::Tail(n) => {
            let len = lines.len();
            lines.into_iter().skip(len.saturating_sub(*n)).collect()
        }
        Stage::MaxLines(n) => {
            if lines.len() <= *n {
                lines
            } else {
                let dropped = lines.len() - n;
                let mut kept: Vec<String> = lines.into_iter().take(*n).collect();
                kept.push(format!("[squeez: {} more lines dropped by max_lines]", dropped));
                kept
            }
        }
        Stage::OnEmpty(text) => {
            if lines.is_empty() {
                vec![text.clone()]
            } else {
                lines
            }
        }
    }
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            while let Some(&nc) = chars.peek() {
                chars.next();
                if nc.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ── Layered lookup ──────────────────────────────────────────────────────

fn project_filters_path() -> PathBuf {
    PathBuf::from(".squeez").join("filters.ini")
}

fn user_filters_path() -> PathBuf {
    crate::session::squeez_dir().join("filters.ini")
}

/// Loads and merges all layers, USER entries first and PROJECT entries
/// last: `find_in` breaks a same-length-name tie by keeping the last
/// element seen (`Iterator::max_by_key`'s documented tie-break), so
/// project must be last in this list for a project def to shadow a
/// same-named user def.
fn load_all() -> Vec<FilterDef> {
    let mut defs = Vec::new();
    if let Ok(content) = std::fs::read_to_string(user_filters_path()) {
        defs.extend(parse(&content));
    }
    if let Ok(content) = std::fs::read_to_string(project_filters_path()) {
        defs.extend(parse(&content));
    }
    defs
}

/// Finds the best-matching filter definition for `cmd`: the longest
/// section name that is a prefix of `cmd`. On a length tie, the
/// project-level definition wins (see `load_all`'s ordering note).
pub fn find_for_command(cmd: &str) -> Option<FilterDef> {
    find_in(&load_all(), cmd)
}

fn find_in(defs: &[FilterDef], cmd: &str) -> Option<FilterDef> {
    defs.iter()
        .filter(|d| cmd.starts_with(d.name.as_str()))
        .max_by_key(|d| d.name.len())
        .cloned()
}

// ── `squeez filter-test` ─────────────────────────────────────────────────

pub struct TestReport {
    pub filter_name: String,
    pub passed: usize,
    pub failed: Vec<(String, String, String)>, // (input, expected, actual)
}

pub fn run_tests(defs: &[FilterDef]) -> Vec<TestReport> {
    defs.iter()
        .filter(|d| !d.tests.is_empty())
        .map(|d| {
            let mut passed = 0;
            let mut failed = Vec::new();
            for (input, expected) in &d.tests {
                let lines: Vec<String> = input.lines().map(String::from).collect();
                let actual = apply(d, lines).join("\n");
                if &actual == expected {
                    passed += 1;
                } else {
                    failed.push((input.clone(), expected.clone(), actual));
                }
            }
            TestReport { filter_name: d.name.clone(), passed, failed }
        })
        .collect()
}

pub fn format_reports(reports: &[TestReport]) -> String {
    if reports.is_empty() {
        return "squeez filter-test: no filters with inline tests found.\n".to_string();
    }
    let mut out = String::new();
    let mut total_pass = 0;
    let mut total_fail = 0;
    for r in reports {
        for (input, expected, actual) in &r.failed {
            out.push_str(&format!(
                "FAIL [{}]\n  input:    {:?}\n  expected: {:?}\n  actual:   {:?}\n",
                r.filter_name, input, expected, actual
            ));
        }
        total_pass += r.passed;
        total_fail += r.failed.len();
        out.push_str(&format!("[{}] {} passed, {} failed\n", r.filter_name, r.passed, r.failed.len()));
    }
    out.push_str(&format!("\n{} passed, {} failed total\n", total_pass, total_fail));
    out
}

/// CLI entry point for `squeez filter-test`.
pub fn run_cli() -> i32 {
    let defs = load_all();
    let reports = run_tests(&defs);
    let any_failed = reports.iter().any(|r| !r.failed.is_empty());
    print!("{}", format_reports(&reports));
    if any_failed {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(content: &str) -> FilterDef {
        let defs = parse(content);
        assert_eq!(defs.len(), 1, "expected exactly one filter def, got {:?}", defs);
        defs.into_iter().next().unwrap()
    }

    #[test]
    fn parses_section_header_and_stages() {
        let ini = r#"
[filter "cargo tree"]
strip_ansi = true
strip_lines_containing = "├──"
head = 20
"#;
        let d = one(ini);
        assert_eq!(d.name, "cargo tree");
        assert_eq!(
            d.stages,
            vec![
                Stage::StripAnsi,
                Stage::StripLinesContaining("├──".to_string()),
                Stage::Head(20),
            ]
        );
    }

    #[test]
    fn parses_replace_and_match_line_with_unless() {
        let ini = r#"
[filter "mytool"]
replace = "DEBUG: " -> ""
match_line = "error" unless "warning: unused"
"#;
        let d = one(ini);
        assert_eq!(
            d.stages[0],
            Stage::Replace { from: "DEBUG: ".to_string(), to: "".to_string() }
        );
        assert_eq!(
            d.stages[1],
            Stage::MatchLine {
                pattern: "error".to_string(),
                unless: Some("warning: unused".to_string())
            }
        );
    }

    #[test]
    fn ignores_directives_before_any_section() {
        let ini = "head = 5\n[filter \"x\"]\ntail = 3\n";
        let d = one(ini);
        assert_eq!(d.stages, vec![Stage::Tail(3)]);
    }

    #[test]
    fn unknown_directive_skipped_not_fatal() {
        let ini = "[filter \"x\"]\nbogus_stage = whatever\nhead = 2\n";
        let d = one(ini);
        assert_eq!(d.stages, vec![Stage::Head(2)]);
    }

    #[test]
    fn glob_match_modes() {
        assert!(glob_match("error", "an error occurred"));
        assert!(!glob_match("error", "all good"));
        assert!(glob_match("error*", "error: bad thing"));
        assert!(!glob_match("error*", "an error: bad thing"));
        assert!(glob_match("*.rs", "src/main.rs"));
        assert!(!glob_match("*.rs", "src/main.py"));
        assert!(glob_match("*ERROR*value*", "2024 ERROR bad value here"));
        assert!(glob_match("*", "anything at all"));
    }

    #[test]
    fn pipeline_strips_ansi_then_filters_then_heads() {
        let ini = r#"
[filter "noisy"]
strip_ansi = true
strip_lines_containing = "DEBUG"
head = 2
"#;
        let d = one(ini);
        let lines = vec![
            "\x1b[32mkeep me\x1b[0m".to_string(),
            "DEBUG: noise".to_string(),
            "another keeper".to_string(),
            "DEBUG: more noise".to_string(),
            "third keeper (dropped by head)".to_string(),
        ];
        let out = apply(&d, lines);
        assert_eq!(out, vec!["keep me".to_string(), "another keeper".to_string()]);
    }

    #[test]
    fn match_line_unless_guard_prevents_swallowing_errors() {
        let ini = r#"
[filter "quiet"]
match_line = "INFO" unless "ERROR"
"#;
        let d = one(ini);
        let lines = vec![
            "INFO: starting up".to_string(),
            "DEBUG: noise".to_string(),
            "ERROR: something broke".to_string(), // doesn't match "INFO" but guard keeps it
        ];
        let out = apply(&d, lines);
        assert_eq!(
            out,
            vec!["INFO: starting up".to_string(), "ERROR: something broke".to_string()]
        );
    }

    #[test]
    fn max_lines_caps_and_annotates() {
        let ini = "[filter \"x\"]\nmax_lines = 3\n";
        let d = one(ini);
        let lines: Vec<String> = (0..10).map(|i| format!("line {}", i)).collect();
        let out = apply(&d, lines);
        assert_eq!(out.len(), 4); // 3 kept + 1 annotation
        assert!(out[3].contains("7 more lines dropped"));
    }

    #[test]
    fn on_empty_substitutes_when_pipeline_empties_input() {
        let ini = r#"
[filter "x"]
strip_lines_containing = "*"
on_empty = "(nothing to show)"
"#;
        let d = one(ini);
        let out = apply(&d, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(out, vec!["(nothing to show)".to_string()]);
    }

    #[test]
    fn truncate_line_at_drops_marker_and_tail() {
        let ini = "[filter \"x\"]\ntruncate_line_at = \" (\"\n";
        let d = one(ini);
        let out = apply(&d, vec!["error at foo.rs (line 10, col 5)".to_string()]);
        assert_eq!(out, vec!["error at foo.rs".to_string()]);
    }

    #[test]
    fn find_for_command_prefers_longest_match() {
        let defs = vec![
            FilterDef { name: "cargo".to_string(), stages: vec![Stage::Head(1)], tests: vec![] },
            FilterDef {
                name: "cargo tree".to_string(),
                stages: vec![Stage::Head(2)],
                tests: vec![],
            },
        ];
        let found = find_in(&defs, "cargo tree --depth 2").unwrap();
        assert_eq!(found.name, "cargo tree");
        let found2 = find_in(&defs, "cargo build --release").unwrap();
        assert_eq!(found2.name, "cargo");
    }

    #[test]
    fn find_for_command_none_when_no_match() {
        let defs = vec![FilterDef { name: "cargo tree".to_string(), stages: vec![], tests: vec![] }];
        assert!(find_in(&defs, "npm install").is_none());
    }

    #[test]
    fn inline_tests_pass_and_fail_reported() {
        let ini = r#"
[filter "greet"]
replace = "hello" -> "hi"
test_input = "hello world"
test_expect = "hi world"
test_input = "hello there"
test_expect = "WRONG EXPECTATION"
"#;
        let d = one(ini);
        assert_eq!(d.tests.len(), 2);
        let reports = run_tests(&[d]);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].passed, 1);
        assert_eq!(reports[0].failed.len(), 1);
        let out = format_reports(&reports);
        assert!(out.contains("FAIL"));
        assert!(out.contains("1 passed, 1 failed"));
    }

    #[test]
    fn filters_with_no_tests_excluded_from_reports() {
        let ini = "[filter \"x\"]\nhead = 1\n";
        let d = one(ini);
        let reports = run_tests(&[d]);
        assert!(reports.is_empty());
        assert!(format_reports(&reports).contains("no filters with inline tests"));
    }
}
