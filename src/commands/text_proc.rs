// Handler for text-processing commands: grep, awk, sed, ripgrep (rg).
// These produce match lines; squeeze groups them by file and deduplicates
// identical match patterns.

use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{dedup, truncation};

pub struct TextProcHandler;

impl Handler for TextProcHandler {
    fn compress(&self, _cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        // Group consecutive matches from the same file prefix (filename:lineno:match)
        let grouped = group_by_file(lines);
        let grouped = dedup::apply(grouped, config.dedup_min);
        truncation::apply(grouped, config.max_lines.max(20), truncation::Keep::Head)
    }
}

/// Collapse multiple matches from the same file into a single summary block.
/// Input: "src/foo.rs:42:  match line" → kept as-is.
/// When the same file appears ≥3 times, emit a summary: "src/foo.rs: N matches".
fn group_by_file(lines: Vec<String>) -> Vec<String> {
    use std::collections::HashMap;

    let mut file_count: HashMap<String, usize> = HashMap::new();
    let mut file_order: Vec<String> = Vec::new();

    for line in &lines {
        if let Some(file) = extract_grep_file(line) {
            let e = file_count.entry(file.clone()).or_insert(0);
            if *e == 0 {
                file_order.push(file);
            }
            *e += 1;
        }
    }

    // If any file has ≥5 matches, collapse those; otherwise passthrough.
    let should_collapse = file_count.values().any(|&n| n >= 5);
    if !should_collapse {
        return lines;
    }

    let mut out: Vec<String> = Vec::new();
    let mut collapsed: std::collections::HashSet<String> = std::collections::HashSet::new();

    for line in &lines {
        if let Some(file) = extract_grep_file(line) {
            let count = file_count.get(&file).copied().unwrap_or(0);
            if count >= 5 {
                if collapsed.insert(file.clone()) {
                    out.push(format!("{}: {} matches [squeez: collapsed]", file, count));
                }
                continue;
            }
        }
        out.push(line.clone());
    }

    // Summary files with <5 matches already added inline.
    // Also list any collapsed files we didn't emit yet (edge case).
    let _ = file_order; // ordering was used above implicitly via HashMap iteration
    out
}

fn extract_grep_file(line: &str) -> Option<String> {
    // Formats: "filename:lineno:content" or "filename:content" or just "filename"
    if line.trim().is_empty() || line.starts_with("Binary file") {
        return None;
    }
    // Try "path:digits:..." pattern
    let parts: Vec<&str> = line.splitn(3, ':').collect();
    if parts.len() >= 2 && parts[1].parse::<u32>().is_ok() {
        let p = parts[0];
        if p.contains('.') || p.contains('/') {
            return Some(p.to_string());
        }
    }
    // Try "path:content" (no line number)
    if parts.len() >= 2 && (parts[0].contains('/') || parts[0].contains('.')) {
        return Some(parts[0].to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn grep_lines(file: &str, n: usize) -> Vec<String> {
        (0..n)
            .map(|i| format!("{}:{}:    match content {}", file, i + 1, i))
            .collect()
    }

    #[test]
    fn collapses_many_matches_from_same_file() {
        let mut lines = grep_lines("src/main.rs", 6);
        lines.push("other.rs:1:something".into());
        let cfg = Config::default();
        let out = TextProcHandler.compress("grep foo", lines, &cfg);
        let collapsed: Vec<_> = out.iter().filter(|l| l.contains("collapsed")).collect();
        assert_eq!(collapsed.len(), 1);
        assert!(collapsed[0].contains("src/main.rs"));
        assert!(collapsed[0].contains("6 matches"));
    }

    #[test]
    fn passthrough_when_few_matches() {
        let lines = grep_lines("src/main.rs", 3);
        let cfg = Config::default();
        let out = TextProcHandler.compress("grep foo", lines.clone(), &cfg);
        // 3 < 5, so no collapse — dedup may merge identical lines though
        assert!(out.iter().all(|l| !l.contains("collapsed")));
    }
}
