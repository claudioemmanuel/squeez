//! Structured condenser for `ruff check --output-format json` output — a
//! flat top-level array of individual violations (unlike eslint's per-file
//! grouping), each carrying `filename`, `code`, `location`, `message`.
//!
//! Gated on the command name containing "ruff" for the same reason as
//! eslint_json: an empty `[]` array has no distinguishing content on its own.

use crate::json_util::JsonValue;
use crate::json_util;

const MAX_CODE_GROUPS_PER_FILE: usize = 20;

pub fn condense(cmd: &str, lines: &[String]) -> Option<Vec<String>> {
    if !cmd.to_ascii_lowercase().contains("ruff") {
        return None;
    }
    let joined = lines.join("\n");
    let start = joined.find('[')?;
    let end = joined.rfind(']')?;
    if end <= start {
        return None;
    }
    let slice = &joined[start..=end];
    if !(slice.contains("\"code\"") && slice.contains("\"filename\"")) && slice.trim() != "[]" {
        return None;
    }
    let root = json_util::parse_value(slice)?;
    let violations = match root {
        JsonValue::Arr(items) => items,
        _ => return None,
    };

    if violations.is_empty() {
        return Some(vec!["ok 0 problems".to_string()]);
    }

    // Group by filename, preserving first-seen order, then by code within a file.
    let mut files: Vec<(String, Vec<(String, u64, Option<(u64, u64)>)>)> = Vec::new();
    for v in &violations {
        let filename = v.get("filename").and_then(|x| x.as_str()).unwrap_or("<file>").to_string();
        let code = v.get("code").and_then(|x| x.as_str()).unwrap_or("?").to_string();
        let row = v.get("location").and_then(|l| l.get("row")).and_then(|x| x.as_u64());
        let col = v.get("location").and_then(|l| l.get("column")).and_then(|x| x.as_u64());

        let file_entry = match files.iter_mut().find(|(f, _)| *f == filename) {
            Some(e) => e,
            None => {
                files.push((filename, Vec::new()));
                files.last_mut().unwrap()
            }
        };
        match file_entry.1.iter_mut().find(|(c, ..)| *c == code) {
            Some(g) => g.1 += 1,
            None => file_entry.1.push((code, 1, row.zip(col))),
        }
    }

    let mut out = Vec::new();
    out.push(format!(
        "FAIL {} problems — {} file{} affected",
        violations.len(),
        files.len(),
        if files.len() == 1 { "" } else { "s" }
    ));
    for (filename, groups) in &files {
        let total: u64 = groups.iter().map(|(_, n, _)| n).sum();
        out.push(format!(
            "{}: {} problem{}",
            filename,
            total,
            if total == 1 { "" } else { "s" }
        ));
        for (code, count, pos) in groups.iter().take(MAX_CODE_GROUPS_PER_FILE) {
            let loc = pos.map(|(r, c)| format!(" — first at {}:{}", r, c)).unwrap_or_default();
            out.push(format!("  {} ×{}{}", code, count, loc));
        }
        if groups.len() > MAX_CODE_GROUPS_PER_FILE {
            out.push(format!("  +{} more rule groups", groups.len() - MAX_CODE_GROUPS_PER_FILE));
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
    fn non_ruff_cmd_returns_none() {
        let json = r#"[{"code":"F401","filename":"a.py","location":{"row":1,"column":1},"message":"m"}]"#;
        assert!(condense("eslint . --format json", &lines(json)).is_none());
    }

    #[test]
    fn empty_array_collapses_to_ok() {
        let out = condense("ruff check . --output-format json", &lines("[]")).expect("should sniff ruff json");
        assert_eq!(out, vec!["ok 0 problems".to_string()]);
    }

    #[test]
    fn groups_by_file_then_code() {
        let json = r#"[
{"code":"F401","filename":"/repo/src/foo.py","location":{"row":1,"column":1},"message":"`os` imported but unused"},
{"code":"F401","filename":"/repo/src/foo.py","location":{"row":2,"column":1},"message":"`sys` imported but unused"},
{"code":"E501","filename":"/repo/src/foo.py","location":{"row":40,"column":89},"message":"line too long"},
{"code":"F841","filename":"/repo/src/bar.py","location":{"row":5,"column":5},"message":"local variable unused"}
]"#;
        let out = condense("ruff check . --output-format json", &lines(json)).expect("should sniff ruff json");
        assert!(out.iter().any(|l| l.contains("FAIL 4 problems")));
        assert!(out.iter().any(|l| l.contains("/repo/src/foo.py")));
        assert!(out.iter().any(|l| l.contains("F401 ×2") && l.contains("1:1")));
        assert!(out.iter().any(|l| l.contains("E501 ×1") && l.contains("40:89")));
        assert!(out.iter().any(|l| l.contains("/repo/src/bar.py")));
        assert!(out.iter().any(|l| l.contains("F841 ×1")));
    }
}
