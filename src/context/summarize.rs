use crate::commands::wrap;
use crate::config::Config;
use crate::json_util;

/// Which output shape to use for the dense summary.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SummaryFormat {
    /// Multi-line prose (original behaviour).
    Prose,
    /// Single JSON line followed by verbatim tail lines.
    Structured,
}

/// Number of last lines to preserve verbatim in the Prose summary.
const TAIL_KEEP: usize = 20;
/// Tail lines preserved in Structured (JSON) summary — fewer since the JSON
/// envelope already captures errors/files/test status compactly.
const TAIL_KEEP_STRUCTURED: usize = 5;
/// Number of top items to keep per category.
const TOP_N: usize = 5;
/// When the output looks benign (no errors, panics, failures, tracebacks)
/// the summarize trigger is lifted by this factor — i.e. the user gets to
/// see twice as much verbatim output before the dense summary kicks in.
/// Successful builds and clean test runs get twice the threshold so they stay
/// verbatim. Aggressive summarization is reserved for outputs that already
/// contain errors/failures, where head/tail is most useful anyway.
pub const BENIGN_MULTIPLIER: usize = 2;

/// Cheap, allocation-free substring scan for the most common error / failure
/// markers. False negatives (missing exotic-cased markers) are tolerable;
/// false positives just preserve the previous eager threshold for that call.
fn line_has_error_marker(line: &str) -> bool {
    // Compile-time-friendly disjunction. Listed by descending typical frequency.
    line.contains("error:")
        || line.contains("Error:")
        || line.contains("ERROR:")
        || line.contains("error[")
        || line.contains("Error[")
        || line.contains("panic")
        || line.contains("Panic")
        || line.contains("PANIC")
        || line.contains("fatal:")
        || line.contains("Fatal:")
        || line.contains("FATAL:")
        || line.contains("failed")
        || line.contains("Failed")
        || line.contains("FAILED")
        || line.contains("Traceback")
        || line.contains("traceback")
        || line.contains("Exception")
        || line.contains("exception")
}

/// True iff the output contains zero error / failure / traceback markers.
/// Used by `should_apply` to relax the summarize trigger for benign output.
pub fn is_benign(lines: &[String]) -> bool {
    !lines.iter().any(|l| line_has_error_marker(l))
}

/// Decide whether to replace `lines` with a dense summary.
///
/// Threshold is `cfg.summarize_threshold_lines` for outputs that contain any
/// error / failure / traceback marker, and `cfg.summarize_threshold_lines *
/// BENIGN_MULTIPLIER` (default 2×) for benign outputs. The benign relaxation
/// preserves more verbatim text in the common "long but successful build"
/// case while keeping the eager trigger for debugging output.
pub fn should_apply(lines: &[String], cfg: &Config) -> bool {
    let threshold = if is_benign(lines) {
        cfg.summarize_threshold_lines.saturating_mul(BENIGN_MULTIPLIER)
    } else {
        cfg.summarize_threshold_lines
    };
    lines.len() > threshold
}

/// Build a dense ≤40-line summary from a large output (Prose shape).
pub fn apply(lines: Vec<String>, cmd: &str) -> Vec<String> {
    apply_with_format(lines, cmd, SummaryFormat::Prose)
}

/// Build a summary in the requested format.
///
/// * `Prose`      — multi-line key=value output (original behaviour, ≤40 lines).
/// * `Structured` — one compact JSON line + up to TAIL_KEEP verbatim tail lines.
pub fn apply_with_format(lines: Vec<String>, cmd: &str, format: SummaryFormat) -> Vec<String> {
    match format {
        SummaryFormat::Prose => apply_prose(lines, cmd),
        SummaryFormat::Structured => apply_structured(lines, cmd),
    }
}

fn apply_prose(lines: Vec<String>, cmd: &str) -> Vec<String> {
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

fn apply_structured(lines: Vec<String>, cmd: &str) -> Vec<String> {
    let total = lines.len();
    let joined = lines.join("\n");

    let files = wrap::extract_file_paths(&joined);
    let errors = wrap::extract_errors(&joined);
    let test = wrap::extract_test_summary(&joined);

    let cmd_short: String = cmd.chars().take(30).collect();
    let tail_n = TAIL_KEEP_STRUCTURED.min(total);
    let tail_start = total.saturating_sub(tail_n);

    // Build files JSON array (top 5)
    let files_json = {
        let items: Vec<String> = files
            .iter()
            .take(TOP_N)
            .map(|f| format!("\"{}\"", json_util::escape_str(f)))
            .collect();
        format!("[{}]", items.join(","))
    };

    // Build errors JSON array (top 5, each truncated to 120 chars)
    let errors_json = {
        let items: Vec<String> = errors
            .iter()
            .take(TOP_N)
            .map(|e| {
                let trimmed: String = e.chars().take(120).collect();
                format!("\"{}\"", json_util::escape_str(&trimmed))
            })
            .collect();
        format!("[{}]", items.join(","))
    };

    let test_json = json_util::escape_str(&test);

    let json_line = format!(
        "{{\"squeez\":\"summary\",\"cmd\":\"{}\",\"total\":{},\"files\":{},\"errors\":{},\"test\":\"{}\",\"tail\":{}}}",
        json_util::escape_str(&cmd_short),
        total,
        files_json,
        errors_json,
        test_json,
        tail_n,
    );

    let mut out: Vec<String> = Vec::with_capacity(1 + tail_n);
    out.push(json_line);
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
    fn should_apply_under_threshold_false() {
        let c = cfg(); // threshold=100
        let small: Vec<String> = (0..50).map(|i| format!("l{}", i)).collect();
        assert!(!should_apply(&small, &c));
    }

    #[test]
    fn should_apply_eager_for_error_output() {
        // 150 lines with one error marker → non-benign → threshold stays 100
        let c = cfg();
        let mut lines: Vec<String> = (0..150).map(|i| format!("line {}", i)).collect();
        lines.push("error: something broke".to_string());
        assert!(should_apply(&lines, &c));
    }

    #[test]
    fn should_apply_relaxed_for_benign_output() {
        // 150 benign lines → threshold doubles to 200 → does NOT apply
        let c = cfg();
        let lines: Vec<String> = (0..150).map(|i| format!("line {}", i)).collect();
        assert!(!should_apply(&lines, &c));
        // 250 benign lines → exceeds 200 → applies
        let big: Vec<String> = (0..250).map(|i| format!("line {}", i)).collect();
        assert!(should_apply(&big, &c));
    }

    #[test]
    fn benign_detection_recognizes_common_markers() {
        let benign: Vec<String> = vec!["compiling foo".into(), "all good".into()];
        assert!(is_benign(&benign));

        let with_error: Vec<String> = vec!["building".into(), "error: x".into()];
        assert!(!is_benign(&with_error));

        let with_panic: Vec<String> = vec!["thread 'main' panicked at ...".into()];
        assert!(!is_benign(&with_panic));

        let with_traceback: Vec<String> =
            vec!["Traceback (most recent call last):".into(), "  File ...".into()];
        assert!(!is_benign(&with_traceback));

        let with_failure: Vec<String> = vec!["test foo ... FAILED".into()];
        assert!(!is_benign(&with_failure));
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
