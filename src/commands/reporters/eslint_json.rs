//! Structured condenser for `eslint --format json` (and biome's compatible
//! `--reporter=json` shape) output — a top-level array of per-file result
//! objects, each with a `messages` array.
//!
//! Gated on the command name containing "eslint" (in addition to the content
//! shape) because an empty `[]` array carries no distinguishing content —
//! without the command-name gate an unrelated tool's empty JSON array could
//! be misread as "0 lint problems".

use crate::json_util::JsonValue;
use crate::json_util;

const MAX_RULE_GROUPS_PER_FILE: usize = 20;
const MAX_CLEAN_FILES_NAMED: usize = 20;

pub fn condense(cmd: &str, lines: &[String]) -> Option<Vec<String>> {
    let lower = cmd.to_ascii_lowercase();
    // `next lint` wraps eslint and emits the same JSON with --format json.
    if !lower.contains("eslint") && !lower.contains("next lint") {
        return None;
    }
    let joined = lines.join("\n");
    let start = joined.find('[')?;
    let end = joined.rfind(']')?;
    if end <= start {
        return None;
    }
    let slice = &joined[start..=end];
    if !slice.contains("\"messages\"") && slice.trim() != "[]" {
        return None;
    }
    let root = json_util::parse_value(slice)?;
    let files = match root {
        JsonValue::Arr(items) => items,
        _ => return None,
    };

    let mut total_errors = 0u64;
    let mut total_warnings = 0u64;
    let mut clean_files: Vec<String> = Vec::new();
    let mut file_reports: Vec<String> = Vec::new();

    for f in &files {
        let path = f.get("filePath").and_then(|v| v.as_str()).unwrap_or("<file>");
        let errors = f.get("errorCount").and_then(|v| v.as_u64()).unwrap_or(0);
        let warnings = f.get("warningCount").and_then(|v| v.as_u64()).unwrap_or(0);
        total_errors += errors;
        total_warnings += warnings;

        if errors == 0 && warnings == 0 {
            clean_files.push(path.to_string());
            continue;
        }

        file_reports.push(format!(
            "{}: {} problem{} ({} error{}, {} warning{})",
            path,
            errors + warnings,
            if errors + warnings == 1 { "" } else { "s" },
            errors,
            if errors == 1 { "" } else { "s" },
            warnings,
            if warnings == 1 { "" } else { "s" }
        ));

        // Group messages by ruleId, preserving first-seen order.
        let mut groups: Vec<(String, u64, bool, Option<(u64, u64)>)> = Vec::new(); // (rule, count, is_error, first line/col)
        for m in f.get("messages").map(|v| v.as_arr()).unwrap_or(&[]) {
            let rule = m
                .get("ruleId")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "parse-error".to_string());
            let severity_is_error = m.get("severity").and_then(|v| v.as_u64()).unwrap_or(1) >= 2;
            let line = m.get("line").and_then(|v| v.as_u64());
            let col = m.get("column").and_then(|v| v.as_u64());
            match groups.iter_mut().find(|(r, ..)| *r == rule) {
                Some(g) => g.1 += 1,
                None => groups.push((
                    rule,
                    1,
                    severity_is_error,
                    line.zip(col),
                )),
            }
        }
        for (rule, count, is_error, pos) in groups.iter().take(MAX_RULE_GROUPS_PER_FILE) {
            let loc = pos.map(|(l, c)| format!(" — first at {}:{}", l, c)).unwrap_or_default();
            file_reports.push(format!(
                "  {} ×{} [{}]{}",
                rule,
                count,
                if *is_error { "error" } else { "warning" },
                loc
            ));
        }
        if groups.len() > MAX_RULE_GROUPS_PER_FILE {
            file_reports.push(format!("  +{} more rule groups", groups.len() - MAX_RULE_GROUPS_PER_FILE));
        }
    }

    let mut out = Vec::new();
    if total_errors == 0 && total_warnings == 0 {
        out.push(format!("ok {} files, 0 problems", files.len()));
        return Some(out);
    }

    out.push(format!(
        "FAIL {} problems ({} errors, {} warnings) — {}/{} files",
        total_errors + total_warnings,
        total_errors,
        total_warnings,
        files.len() - clean_files.len(),
        files.len()
    ));
    out.extend(file_reports);

    if !clean_files.is_empty() {
        if clean_files.len() <= MAX_CLEAN_FILES_NAMED {
            out.push(format!("clean: {}", clean_files.join(", ")));
        } else {
            out.push(format!(
                "clean: {}, +{} more",
                clean_files[..MAX_CLEAN_FILES_NAMED].join(", "),
                clean_files.len() - MAX_CLEAN_FILES_NAMED
            ));
        }
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(|l| l.to_string()).collect()
    }

    #[test]
    fn non_eslint_cmd_returns_none() {
        let json = r#"[{"filePath":"a.ts","errorCount":1,"warningCount":0,"messages":[]}]"#;
        assert!(condense("ruff check .", &lines(json)).is_none());
    }

    #[test]
    fn all_clean_collapses_to_one_line() {
        let json = r#"[{"filePath":"a.ts","errorCount":0,"warningCount":0,"messages":[]},{"filePath":"b.ts","errorCount":0,"warningCount":0,"messages":[]}]"#;
        let out = condense("eslint . --format json", &lines(json)).expect("should sniff eslint json");
        assert_eq!(out.len(), 1);
        assert!(out[0].contains("ok 2 files, 0 problems"));
    }

    #[test]
    fn groups_messages_by_rule_and_preserves_names() {
        let json = r#"[{"filePath":"/repo/src/foo.ts","errorCount":2,"warningCount":1,"messages":[{"ruleId":"no-unused-vars","severity":2,"message":"x unused","line":10,"column":5},{"ruleId":"no-unused-vars","severity":2,"message":"y unused","line":20,"column":1},{"ruleId":"eqeqeq","severity":1,"message":"use ===","line":22,"column":3}]},{"filePath":"/repo/src/bar.ts","errorCount":0,"warningCount":0,"messages":[]}]"#;
        let out = condense("eslint . --format json", &lines(json)).expect("should sniff eslint json");
        assert!(out.iter().any(|l| l.contains("FAIL 3 problems")));
        assert!(out.iter().any(|l| l.contains("/repo/src/foo.ts")));
        assert!(out.iter().any(|l| l.contains("no-unused-vars ×2 [error]") && l.contains("10:5")));
        assert!(out.iter().any(|l| l.contains("eqeqeq ×1 [warning]")));
        assert!(out.iter().any(|l| l.contains("clean: /repo/src/bar.ts")));
    }
}
