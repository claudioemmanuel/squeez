//! Information-preservation scoring — detect over-compression that would force
//! Claude to re-investigate.
//!
//! Background: the rtk competitor regression (rtk-ai/rtk#582) showed that
//! aggressive bash-output compression which strips structural markers (file
//! paths, line numbers, error codes) causes the LLM to generate 50% more
//! output tokens trying to explain or re-derive what was removed — a net
//! +18% total cost despite "winning" on input tokens.
//!
//! This module extracts structural anchors from baseline output and measures
//! what fraction survives compression. Distinct from `quality_score` (which
//! looks at unique diagnostic words): preservation focuses on the
//! navigation-critical references the model uses to decide whether further
//! tool calls are needed.
//!
//! All extraction is byte-scan based, zero-dep, deterministic.
//!
//! References:
//! - <https://github.com/rtk-ai/rtk/issues/582>

use std::collections::HashSet;

/// Reduction threshold above which preservation matters most.
pub const RISK_REDUCTION_THRESHOLD: f64 = 90.0;
/// Preservation floor below which a high-reduction scenario is flagged.
pub const RISK_PRESERVATION_FLOOR: f64 = 0.70;

/// Structural anchors extracted from output.
///
/// These are the byte patterns Claude relies on to decide whether to make
/// follow-up tool calls. Losing them forces re-investigation.
#[derive(Debug, Default, Clone)]
pub struct Anchors {
    /// `path/to/file.ext` references (with extension).
    pub file_paths: HashSet<String>,
    /// `path/to/file.ext:NN` or `path/to/file.ext(NN,MM)` positions.
    pub line_refs: HashSet<String>,
    /// `error[E0432]`, `TS2345`, `error:`, `FAIL`, `panic` markers.
    pub error_markers: HashSet<String>,
    /// `N passed`, `N failed`, `N errors`, `N warnings` counts.
    pub test_verdicts: HashSet<String>,
}

impl Anchors {
    /// Total anchor count — denominator for preservation scoring.
    pub fn total(&self) -> usize {
        self.file_paths.len()
            + self.line_refs.len()
            + self.error_markers.len()
            + self.test_verdicts.len()
    }
}

/// Extract structural anchors from text. Pure, deterministic, zero-dep.
pub fn extract_anchors(text: &str) -> Anchors {
    let mut out = Anchors::default();
    for line in text.lines() {
        scan_line(line, &mut out);
    }
    out
}

fn scan_line(line: &str, anchors: &mut Anchors) {
    // ── Test verdicts: "N passed", "N failed", "N errors", "N warnings" ─────
    // Anchor on count+keyword to avoid matching bare keywords in prose.
    for (kw, _) in &[
        ("passed", 6usize),
        ("failed", 6),
        ("errors", 6),
        ("warnings", 8),
    ] {
        if let Some(pos) = line.find(kw) {
            // Walk back to find preceding integer.
            let prefix = &line[..pos];
            let trimmed = prefix.trim_end();
            let mut digit_start = trimmed.len();
            for (i, c) in trimmed.char_indices().rev() {
                if c.is_ascii_digit() {
                    digit_start = i;
                } else {
                    break;
                }
            }
            if digit_start < trimmed.len() {
                let num = &trimmed[digit_start..];
                anchors
                    .test_verdicts
                    .insert(format!("{} {}", num, kw));
            }
        }
    }

    // ── Error markers ───────────────────────────────────────────────────────
    let lower = line.to_ascii_lowercase();
    // error[E0432] / error[TS2345] bracketed style
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' {
            let mut j = i + 1;
            let start = j;
            while j < bytes.len() && bytes[j].is_ascii_alphanumeric() {
                j += 1;
            }
            if j > start + 2 && j < bytes.len() && bytes[j] == b']' {
                let code = &line[start..j];
                let has_alpha = code.bytes().any(|b| b.is_ascii_alphabetic());
                let has_digit = code.bytes().any(|b| b.is_ascii_digit());
                if has_alpha && has_digit {
                    anchors.error_markers.insert(code.to_string());
                }
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    // Standalone error codes when line is diagnostic (TS2345, E0432, EACCES, …)
    let is_diagnostic_line = lower.contains("error")
        || lower.contains("warning")
        || lower.contains("fail")
        || lower.contains("fatal")
        || lower.contains("panic");
    if is_diagnostic_line {
        for tok in line.split(|c: char| !c.is_ascii_alphanumeric()) {
            if tok.len() < 3 || tok.len() > 8 {
                continue;
            }
            let tb = tok.as_bytes();
            let mut alpha = 0usize;
            while alpha < tb.len() && tb[alpha].is_ascii_uppercase() {
                alpha += 1;
            }
            if alpha == 0 || alpha > 4 {
                continue;
            }
            let rest = &tok[alpha..];
            if rest.len() >= 2 && rest.chars().all(|c| c.is_ascii_digit()) {
                anchors.error_markers.insert(tok.to_string());
            }
        }
    }
    // Generic diagnostic keywords
    for kw in &["error:", "fatal:", "panic", "FAIL"] {
        let needle_lc = kw.to_ascii_lowercase();
        if lower.contains(&needle_lc) {
            anchors.error_markers.insert(kw.to_string());
        }
    }

    // ── File paths and line refs ────────────────────────────────────────────
    for tok in line.split(|c: char| {
        c.is_whitespace() || c == '(' || c == ')' || c == ',' || c == ';' || c == '"'
    }) {
        if tok.is_empty() {
            continue;
        }
        // Strip leading/trailing punctuation
        let tok = tok.trim_start_matches(|c: char| {
            !(c.is_ascii_alphanumeric() || c == '/' || c == '.' || c == '_' || c == '-')
        });
        let tok = tok.trim_end_matches(|c: char| {
            !(c.is_ascii_alphanumeric() || c == ')')
        });
        if tok.len() < 4 {
            continue;
        }
        // path/file.ext or path/file.ext:NN
        let has_slash = tok.contains('/') || tok.contains('\\');
        let has_dot = tok.contains('.');
        if !has_dot {
            continue;
        }
        // Reject things like "http://..." or "v1.2.3"
        if tok.starts_with("http://") || tok.starts_with("https://") {
            continue;
        }
        // Try to peel trailing `:LINE` or `:LINE:COL` to expose the path root.
        let (root, line_ref_present) = peel_line_ref(tok);
        if !has_extension_like(root) {
            continue;
        }
        if line_ref_present {
            anchors.line_refs.insert(tok.to_string());
            anchors.file_paths.insert(root.to_string());
            continue;
        }
        // No line ref — record path/file alone
        if has_slash {
            anchors.file_paths.insert(tok.to_string());
        } else if has_dot && tok.chars().any(|c| c.is_ascii_alphabetic()) {
            anchors.file_paths.insert(tok.to_string());
        }
    }

    // ── Parenthesised position: file.ext(LINE,COL) (TypeScript style) ───────
    let mut start = 0;
    while let Some(open) = line[start..].find('(') {
        let abs_open = start + open;
        if let Some(close_rel) = line[abs_open..].find(')') {
            let abs_close = abs_open + close_rel;
            let inside = &line[abs_open + 1..abs_close];
            if !inside.is_empty()
                && inside
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == ',')
                && inside.contains(',')
            {
                // Walk back from open to find filename
                let prefix = &line[..abs_open];
                let last_space = prefix
                    .rfind(|c: char| c.is_whitespace())
                    .map(|i| i + 1)
                    .unwrap_or(0);
                let path = &prefix[last_space..];
                if has_extension_like(path) {
                    anchors
                        .line_refs
                        .insert(format!("{}({})", path, inside));
                    anchors.file_paths.insert(path.to_string());
                }
            }
            start = abs_close + 1;
        } else {
            break;
        }
    }
}

/// Peel up to two trailing `:DIGITS` segments (covers `path:LINE` and
/// `path:LINE:COL`). Returns `(root_without_suffix, suffix_was_present)`.
fn peel_line_ref(tok: &str) -> (&str, bool) {
    let mut peeled = false;
    let mut cur = tok;
    for _ in 0..2 {
        if let Some(pos) = cur.rfind(':') {
            let suffix = &cur[pos + 1..];
            if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
                cur = &cur[..pos];
                peeled = true;
                continue;
            }
        }
        break;
    }
    (cur, peeled)
}

/// Heuristic: does this token end with a plausible file extension?
fn has_extension_like(tok: &str) -> bool {
    let last_dot = match tok.rfind('.') {
        Some(p) => p,
        None => return false,
    };
    let ext = &tok[last_dot + 1..];
    if ext.is_empty() || ext.len() > 6 {
        return false;
    }
    // Ext must be all alphanumeric and contain at least one letter
    let mut has_alpha = false;
    for c in ext.chars() {
        if !c.is_ascii_alphanumeric() {
            return false;
        }
        if c.is_ascii_alphabetic() {
            has_alpha = true;
        }
    }
    has_alpha
}

/// Compute information-preservation score in `[0.0, 1.0]`.
///
/// Returns `1.0` when baseline has no structural anchors (trivially
/// preserved — there was nothing critical to lose).
pub fn info_preservation(baseline: &str, compressed: &str) -> f64 {
    let baseline_anchors = extract_anchors(baseline);
    let total = baseline_anchors.total();
    if total == 0 {
        return 1.0;
    }
    let compressed_anchors = extract_anchors(compressed);
    let preserved = baseline_anchors.file_paths.intersection(&compressed_anchors.file_paths).count()
        + baseline_anchors.line_refs.intersection(&compressed_anchors.line_refs).count()
        + baseline_anchors.error_markers.intersection(&compressed_anchors.error_markers).count()
        + baseline_anchors.test_verdicts.intersection(&compressed_anchors.test_verdicts).count();
    preserved as f64 / total as f64
}

/// True when reduction is aggressive AND preservation has dropped below the
/// floor — the regime where rtk regression occurred.
pub fn is_compression_risk(reduction_pct: f64, preservation: f64) -> bool {
    reduction_pct >= RISK_REDUCTION_THRESHOLD && preservation < RISK_PRESERVATION_FLOOR
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_rust_error_anchors() {
        let text = "error[E0432]: unresolved import `crate::missing`\n --> src/main.rs:3:5\n  |\n3 | use crate::missing;\n";
        let a = extract_anchors(text);
        assert!(a.error_markers.contains("E0432"), "{:?}", a.error_markers);
        assert!(a.line_refs.iter().any(|l| l.contains("src/main.rs:3")));
        assert!(a.file_paths.iter().any(|f| f == "src/main.rs"));
    }

    #[test]
    fn extracts_typescript_paren_position() {
        let text = "src/components/Button.tsx(12,5): error TS2345: Argument type mismatch.\n";
        let a = extract_anchors(text);
        assert!(a.error_markers.contains("TS2345"), "{:?}", a.error_markers);
        assert!(a.line_refs.iter().any(|l| l.contains("(12,5)")), "{:?}", a.line_refs);
        assert!(a.file_paths.iter().any(|f| f == "src/components/Button.tsx"));
    }

    #[test]
    fn extracts_test_verdicts() {
        let text = "test result: ok. 42 passed; 3 failed; 1 ignored\n";
        let a = extract_anchors(text);
        assert!(a.test_verdicts.iter().any(|v| v == "42 passed"), "{:?}", a.test_verdicts);
        assert!(a.test_verdicts.iter().any(|v| v == "3 failed"));
    }

    #[test]
    fn full_preservation_when_identical() {
        let text = "error[E0308]: mismatched types\n --> src/lib.rs:42:10\n";
        let score = info_preservation(text, text);
        assert!((score - 1.0).abs() < 1e-9);
    }

    #[test]
    fn zero_preservation_when_stripped() {
        let baseline = "error[E0308]: mismatched types --> src/lib.rs:42:10\nerror[E0432]: unresolved --> src/main.rs:3:5\n";
        let compressed = "[squeez: 2 errors elided]";
        let score = info_preservation(baseline, compressed);
        assert!(score < 0.1, "got {}", score);
    }

    #[test]
    fn partial_preservation_for_partial_compression() {
        let baseline = "error[E0308]: mismatched --> src/lib.rs:42:10\nerror[E0432]: unresolved --> src/main.rs:3:5\nerror[E0277]: trait --> src/foo.rs:99:1\n";
        let compressed = "error[E0308]: mismatched --> src/lib.rs:42:10\n[squeez: 2 more errors elided]";
        let score = info_preservation(baseline, compressed);
        // 1/3 errors + 1/3 files + 1/3 line_refs preserved → ~0.33
        assert!(score > 0.2 && score < 0.5, "got {}", score);
    }

    #[test]
    fn no_anchors_means_trivially_preserved() {
        let baseline = "Loading...\nDone.\n";
        let compressed = "Done.";
        let score = info_preservation(baseline, compressed);
        assert!((score - 1.0).abs() < 1e-9);
    }

    #[test]
    fn risk_flag_triggers_only_on_high_reduction_and_low_preservation() {
        assert!(is_compression_risk(95.0, 0.5));
        assert!(!is_compression_risk(95.0, 0.85)); // high preservation → safe
        assert!(!is_compression_risk(70.0, 0.3));  // low reduction → not a concern
        assert!(!is_compression_risk(89.9, 0.0));  // below threshold
    }

    #[test]
    fn rejects_urls_and_versions() {
        let text = "fetching https://api.example.com/v1.2.3/data and version 1.2.3\n";
        let a = extract_anchors(text);
        assert!(a.file_paths.is_empty(), "{:?}", a.file_paths);
        assert!(a.line_refs.is_empty(), "{:?}", a.line_refs);
    }
}
