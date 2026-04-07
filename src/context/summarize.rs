use crate::commands::wrap;
use crate::config::Config;

/// Number of last lines to preserve verbatim in the summary.
const TAIL_KEEP: usize = 20;
/// Number of top items to keep per category.
const TOP_N: usize = 5;

pub fn should_apply(lines: &[String], cfg: &Config) -> bool {
    lines.len() > cfg.summarize_threshold_lines
}

/// Build a dense ≤40-line summary from a large output.
pub fn apply(lines: Vec<String>, cmd: &str) -> Vec<String> {
    let total = lines.len();
    let joined = lines.join("\n");

    let files = wrap::extract_file_paths(&joined);
    let errors = wrap::extract_errors(&joined);
    let test = wrap::extract_test_summary(&joined);

    let cmd_short: String = cmd.chars().take(30).collect();

    let mut out: Vec<String> = Vec::with_capacity(40);
    out.push(format!("squeez:summary cmd={}", cmd_short));
    out.push(format!("total_lines={}", total));
    out.push(format!("unique_files={}", files.len()));

    if !errors.is_empty() {
        out.push("top_errors:".to_string());
        for e in errors.iter().take(TOP_N) {
            let trimmed: String = e.chars().take(120).collect();
            out.push(format!("  - {}", trimmed));
        }
    }

    if !files.is_empty() {
        out.push("top_files:".to_string());
        for f in files.iter().take(TOP_N) {
            out.push(format!("  - {}", f));
        }
    }

    if !test.is_empty() {
        out.push(format!("test_summary={}", test));
    }

    let tail_n = TAIL_KEEP.min(total);
    out.push(format!("tail_preserved={}", tail_n));

    let tail_start = total.saturating_sub(tail_n);
    for line in lines.into_iter().skip(tail_start) {
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        let mut c = Config::default();
        c.summarize_threshold_lines = 100;
        c
    }

    #[test]
    fn should_apply_respects_threshold() {
        let c = cfg();
        let small: Vec<String> = (0..50).map(|i| format!("l{}", i)).collect();
        let big: Vec<String> = (0..200).map(|i| format!("l{}", i)).collect();
        assert!(!should_apply(&small, &c));
        assert!(should_apply(&big, &c));
    }

    #[test]
    fn summary_is_bounded() {
        let lines: Vec<String> = (0..5000).map(|i| format!("line {}", i)).collect();
        let out = apply(lines, "cargo build");
        // header (3) + tail header (1) + 20 tail lines = 24
        assert!(out.len() <= 40, "got {} lines", out.len());
    }

    #[test]
    fn summary_preserves_last_20_lines() {
        let lines: Vec<String> = (0..1000).map(|i| format!("line {}", i)).collect();
        let out = apply(lines, "cmd");
        assert!(out.contains(&"line 999".to_string()));
        assert!(out.contains(&"line 980".to_string()));
        assert!(!out.contains(&"line 0".to_string()));
    }

    #[test]
    fn summary_extracts_errors() {
        let mut lines: Vec<String> = (0..600).map(|i| format!("line {}", i)).collect();
        lines.push("error: cannot resolve type".to_string());
        lines.push("error: missing field".to_string());
        let out = apply(lines, "cargo check");
        let joined = out.join("\n");
        assert!(joined.contains("top_errors"));
        assert!(joined.contains("cannot resolve type"));
    }

    #[test]
    fn summary_extracts_files() {
        let mut lines: Vec<String> = (0..600).map(|i| format!("noise {}", i)).collect();
        lines.push("modified: src/main.rs".to_string());
        lines.push("modified: src/lib.rs".to_string());
        let out = apply(lines, "git status");
        let joined = out.join("\n");
        assert!(joined.contains("top_files"));
        assert!(joined.contains("src/main.rs"));
    }

    #[test]
    fn summary_includes_total_count() {
        let lines: Vec<String> = (0..1234).map(|i| format!("l{}", i)).collect();
        let out = apply(lines, "x");
        assert!(out.iter().any(|l| l.contains("total_lines=1234")));
    }
}
