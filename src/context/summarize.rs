use crate::commands::wrap;
use crate::config::Config;
use crate::context::factsheet;
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
        // Quota/plan-limit errors carry no error: prefix (e.g. "You've reached
        // the tool call limit on the Starter plan") but must never classify
        // the output as benign — they are DO-NOT-COMPRESS content the model
        // needs verbatim to change strategy (transcript audit item 7).
        || crate::context::cache::is_quota_error(line)
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
    apply_threshold(lines, cfg.summarize_threshold_lines, cfg.summarize_threshold_bytes)
}

/// Same as `should_apply` but lets the caller override the base threshold for
/// specific tools. Read uses the smaller of `cfg.read_summarize_threshold_lines`
/// and `cfg.summarize_threshold_lines` — typical code files in the 80-300 line
/// range were slipping past the 300-line global default, but a user that
/// lowers the global threshold should still see Read fire at least that early.
pub fn should_apply_for_tool(lines: &[String], cfg: &Config, tool: &str) -> bool {
    let base = match tool {
        "Read" if cfg.read_summarize_threshold_lines > 0 => cfg
            .read_summarize_threshold_lines
            .min(cfg.summarize_threshold_lines),
        _ => cfg.summarize_threshold_lines,
    };
    apply_threshold(lines, base, cfg.summarize_threshold_bytes)
}

/// Fires when EITHER the line count or the total byte size crosses its threshold.
/// The byte trigger catches single-line JSON blobs (`az`, `curl`) that carry
/// tens of KB in one line and would otherwise slip past every line threshold.
/// Both thresholds get the same `BENIGN_MULTIPLIER` relaxation for benign output.
/// `byte_base == 0` disables the byte trigger.
fn apply_threshold(lines: &[String], base: usize, byte_base: usize) -> bool {
    let mult = if is_benign(lines) { BENIGN_MULTIPLIER } else { 1 };
    let line_threshold = base.saturating_mul(mult);
    if lines.len() > line_threshold {
        return true;
    }
    if byte_base > 0 {
        let byte_threshold = byte_base.saturating_mul(mult);
        // +1 per line approximates the newline joiner dropped by the line split.
        let total_bytes: usize =
            lines.iter().map(|l| l.len() + 1).sum::<usize>().saturating_sub(1);
        if total_bytes > byte_threshold {
            return true;
        }
    }
    false
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
    let tail_start = total.saturating_sub(tail_n);

    // R1 factsheet: exact identifiers (SHAs, UUIDs, versions, tickets, big
    // numbers) in the dropped region would otherwise vanish — the tail is
    // preserved verbatim so only lines before it need rescue. Cap the facts
    // so the whole summary stays within the ≤40-line bound.
    let dropped = lines[..tail_start].join("\n");
    let facts = factsheet::extract(&dropped);
    // E4: vocabulary line — top distinctive terms from the dropped region, so
    // a later `squeez_stash_search` query can match this call even though its
    // body isn't kept verbatim. One line, capped at 80 chars.
    let vocab_terms = crate::context::stash_index::distinctive_terms(&dropped, 10);
    let vocab_line = (!vocab_terms.is_empty())
        .then(|| format!("vocab: {}", vocab_terms.join(" ")).chars().take(80).collect::<String>());

    if !facts.is_empty() {
        // room = 40 − lines so far − "ids_preserved:" − vocab line − "tail_preserved=" − tail
        let reserved = 2 + if vocab_line.is_some() { 1 } else { 0 };
        let room = 40usize.saturating_sub(out.len() + reserved + tail_n);
        if room > 0 {
            out.push("ids_preserved:".to_string());
            for f in facts.iter().take(room) {
                out.push(format!("  - {}", f));
            }
        }
    }
    if let Some(v) = vocab_line {
        out.push(v);
    }

    out.push(format!("tail_preserved={}", tail_n));
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

    // R1 factsheet: identifiers from the dropped region (before the tail),
    // empty array when none. Same rationale as the Prose ids_preserved block.
    // No vocab field here (E4 Prose-only) -- Structured's whole point is
    // staying well under Prose's byte size, and the stash_index sidecar
    // already makes the underlying stashed blob searchable independent of
    // what the summary itself carries.
    let dropped = lines[..tail_start].join("\n");
    let ids_json = {
        let items: Vec<String> = factsheet::extract(&dropped)
            .iter()
            .map(|f| format!("\"{}\"", json_util::escape_str(f)))
            .collect();
        format!("[{}]", items.join(","))
    };

    let json_line = format!(
        "{{\"squeez\":\"summary\",\"cmd\":\"{}\",\"total\":{},\"files\":{},\"errors\":{},\"ids\":{},\"test\":\"{}\",\"tail\":{}}}",
        json_util::escape_str(&cmd_short),
        total,
        files_json,
        errors_json,
        ids_json,
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
    fn byte_trigger_fires_on_single_line_json_blob() {
        // 60 KB JSON on ONE line: line count (1) never crosses any line threshold,
        // but the byte trigger fires. Benign → byte threshold doubles to 48 KB.
        let c = cfg(); // summarize_threshold_bytes = 24576 (default)
        let blob = format!("[{}]", "1,".repeat(30_000)); // ~60 KB, benign, 1 line
        assert!(blob.len() > 49_152);
        assert!(should_apply(&[blob], &c));
    }

    #[test]
    fn byte_trigger_ignores_small_multiline_output() {
        // 200 benign lines, ~10 KB total: under the doubled 200-line threshold AND
        // under the doubled 48 KB byte threshold → no regression, no summary.
        let c = cfg();
        let lines: Vec<String> = (0..200).map(|i| format!("line number {}", i)).collect();
        assert!(lines.iter().map(|l| l.len() + 1).sum::<usize>() < 24_576);
        assert!(!should_apply(&lines, &c));
    }

    #[test]
    fn byte_trigger_disabled_when_zero() {
        let mut c = cfg();
        c.summarize_threshold_bytes = 0;
        let blob = format!("[{}]", "1,".repeat(30_000)); // 60 KB, 1 line, benign
        assert!(!should_apply(&[blob], &c));
    }

    #[test]
    fn byte_trigger_eager_for_non_benign_blob() {
        // 30 KB single line WITH an error marker → non-benign → byte threshold
        // stays at 24576 (not doubled) → 30 KB fires.
        let c = cfg();
        let blob = format!("{{\"msg\":\"error: boom\",\"data\":\"{}\"}}", "x".repeat(30_000));
        assert!(should_apply(&[blob], &c));
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
    fn quota_limit_output_is_not_benign() {
        // DO-NOT-COMPRESS (audit item 7): quota errors carry no error: prefix
        // but must keep the eager threshold so they are never relaxed away.
        let with_quota: Vec<String> = vec![
            "fetching node 3109:86".into(),
            "You've reached the Figma MCP tool call limit on the Starter plan.".into(),
        ];
        assert!(!is_benign(&with_quota));

        let with_429: Vec<String> = vec!["HTTP 429 Too Many Requests".into()];
        assert!(!is_benign(&with_429));
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
    fn summarize_vocab_line_present_and_capped() {
        // E4: distinctive terms from the dropped head should surface as a
        // single "vocab: ..." line, capped at 80 chars, in Prose only.
        let mut lines: Vec<String> = Vec::new();
        for _ in 0..40 {
            lines.push("redundancy fuzzy match preservation threshold context noise".to_string());
        }
        for i in 0..600 {
            lines.push(format!("filler {}", i));
        }
        let prose = apply(lines.clone(), "cargo test");
        let vocab_line = prose.iter().find(|l| l.starts_with("vocab: "));
        assert!(vocab_line.is_some(), "expected a vocab line, got: {:?}", prose);
        assert!(vocab_line.unwrap().len() <= 80);
        assert!(vocab_line.unwrap().contains("redundancy"));

        let structured = apply_with_format(lines, "cargo test", SummaryFormat::Structured);
        assert!(
            !structured.iter().any(|l| l.contains("\"vocab\"")),
            "Structured must stay vocab-free to hold its byte-size invariant"
        );
    }

    #[test]
    fn summary_carries_ids_from_dropped_head() {
        // SHA + UUID live in the head, far from the preserved tail — the
        // factsheet must rescue them in both output shapes.
        let mut lines: Vec<String> = vec![
            "commit a1347daf9b2c41e0 built".to_string(),
            "trace 550e8400-e29b-41d4-a716-446655440000 ok".to_string(),
        ];
        lines.extend((0..598).map(|i| format!("noise {}", i)));

        let prose = apply_with_format(lines.clone(), "cmd", SummaryFormat::Prose).join("\n");
        assert!(prose.contains("ids_preserved:"));
        assert!(prose.contains("  - a1347daf9b2c41e0"));
        assert!(prose.contains("  - 550e8400-e29b-41d4-a716-446655440000"));

        let structured = apply_with_format(lines, "cmd", SummaryFormat::Structured);
        assert!(structured[0].contains(
            "\"ids\":[\"550e8400-e29b-41d4-a716-446655440000\",\"a1347daf9b2c41e0\"]"
        ));
    }

    #[test]
    fn summary_includes_total_count() {
        let lines: Vec<String> = (0..1234).map(|i| format!("l{}", i)).collect();
        let out = apply(lines, "x");
        assert!(out.iter().any(|l| l.contains("total_lines=1234")));
    }
}
