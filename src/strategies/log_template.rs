//! Log-template compaction (P2).
//!
//! `dedup` collapses only byte-identical lines. Real logs and stack traces
//! rarely repeat verbatim — they differ by a timestamp, pid, request id, hex
//! hash, or counter. This strategy normalizes those variable tokens to `{}`,
//! groups lines by the resulting template, and collapses a recurring template
//! to a single `[×N] <template>  (e.g. <sample>)` line. The N near-identical
//! lines that carried one bit of information now cost one line.
//!
//! Conservative by design:
//!   - Only collapses a template that actually abstracts a variable — a
//!     template with no `{}` slot is left to `dedup`.
//!   - Only collapses when it recurs at least `min_count` times.
//!   - The collapsed line is emitted at the position of the first occurrence,
//!     preserving order; unique lines pass through verbatim.
//!
//! Zero-dep — hand-rolled token classification, no regex.

use std::collections::{HashMap, HashSet};

const SLOT: &str = "{}";

/// Collapse near-identical lines into templated counts. `min_count` is the
/// recurrence threshold (a template must appear at least this many times).
pub fn apply(lines: Vec<String>, min_count: usize) -> Vec<String> {
    if min_count < 2 || lines.len() < min_count {
        return lines;
    }

    let templates: Vec<String> = lines.iter().map(|l| normalize(l)).collect();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for t in &templates {
        *counts.entry(t.as_str()).or_insert(0) += 1;
    }

    let mut emitted: HashSet<&str> = HashSet::new();
    let mut out = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let t = templates[i].as_str();
        let recurs = counts[t] >= min_count && t.contains(SLOT);
        if recurs {
            if emitted.insert(t) {
                out.push(format!("[×{}] {}  (e.g. {})", counts[t], t, sample(line)));
            }
            // subsequent occurrences are dropped
        } else {
            out.push(line.clone());
        }
    }
    out
}

/// Replace variable-looking whitespace tokens with `{}`. Structure (the
/// non-variable words, and the key in `key=value`) is preserved so the
/// template stays readable.
fn normalize(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut first = true;
    for tok in line.split_whitespace() {
        if !first {
            out.push(' ');
        }
        first = false;
        out.push_str(&templatize(tok));
    }
    out
}

/// Map one token to its template form: the variable part becomes `{}`, but the
/// key of a `key=value` pair is kept (`id=1001` → `id={}`).
fn templatize(tok: &str) -> String {
    if let Some((key, val)) = tok.split_once('=') {
        if !key.is_empty() && !val.is_empty() && looks_like_id(val) {
            return format!("{key}={SLOT}");
        }
    }
    if is_variable(tok) {
        SLOT.to_string()
    } else {
        tok.to_string()
    }
}

/// True if a whole token looks like a variable value (timestamp, number, ip,
/// float, hex/uuid). `key=value` pairs are handled in `templatize`.
fn is_variable(tok: &str) -> bool {
    // Strip surrounding punctuation so `(4821)` and `a3f2,` classify by core.
    let core = tok.trim_matches(|c: char| {
        matches!(c, ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | '<' | '>')
    });
    if core.is_empty() {
        return false;
    }
    looks_like_id(core)
}

/// Heuristic: does `s` look like a numeric / timestamp / hash value?
fn looks_like_id(s: &str) -> bool {
    let digits = s.bytes().filter(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return false; // pure words (incl. ERROR, INFO) are structure, not values
    }
    // Numeric/timestamp/ip/float/date: digits plus the separators that appear
    // *between* digits. Covers 12:30:45, 2026-06-21T10:30:45, 192.168.0.1, 3.14.
    let numeric_shaped = s
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b':' | b'-' | b'/' | b'T' | b'+'));
    if numeric_shaped {
        return true;
    }
    // Hex / uuid id: long, hex-or-dash, with at least a couple of digits so we
    // don't eat ordinary lowercase words.
    let hexish = s.len() >= 8
        && digits >= 2
        && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-');
    if hexish {
        return true;
    }
    // Compound id: an alphanumeric token that carries a digit and a `-`/`_`
    // separator (req-1, pod-abc-7, session_42). Catches the ids that dominate
    // structured logs. Version strings (v1.24.0, 1.2.3) have no `-`/`_`, so
    // they're preserved.
    let compound = digits >= 1
        && s.bytes().any(|b| b == b'-' || b == b'_')
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    compound
}

/// A short, single-line example of the raw line for the collapsed marker.
fn sample(line: &str) -> String {
    const MAX: usize = 100;
    let t = line.trim();
    if t.chars().count() <= MAX {
        t.to_string()
    } else {
        let cut: String = t.chars().take(MAX).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Vec<String> {
        s.lines().map(String::from).collect()
    }

    #[test]
    fn collapses_lines_differing_only_by_variables() {
        let input = v("\
2026-06-21T10:30:01 worker 4821 processing job a3f2c1d4e50
2026-06-21T10:30:02 worker 4822 processing job b1c9d2e3f4a
2026-06-21T10:30:03 worker 4823 processing job d7e0f1a2b3c");
        let out = apply(input, 3);
        assert_eq!(out.len(), 1, "three near-identical lines → one: {out:?}");
        assert!(out[0].starts_with("[×3] "), "{:?}", out[0]);
        assert!(out[0].contains("{}"), "template must carry slots: {:?}", out[0]);
        assert!(out[0].contains("worker"), "structure preserved: {:?}", out[0]);
    }

    #[test]
    fn preserves_unique_lines_and_versions() {
        let input = v("\
starting squeez v1.24.0
listening on port 8787
ready");
        let out = apply(input.clone(), 3);
        assert_eq!(out, input, "no recurring template → unchanged");
    }

    #[test]
    fn leaves_byte_identical_recurrence_to_dedup() {
        // No variable slot → not our job; pass through so dedup handles it.
        let input = v("retrying...\nretrying...\nretrying...");
        let out = apply(input.clone(), 3);
        assert_eq!(out, input);
    }

    #[test]
    fn key_value_pairs_template_on_the_value() {
        let input = v("conn id=1001 ok\nconn id=1002 ok\nconn id=1003 ok\nconn id=1004 ok");
        let out = apply(input, 3);
        assert_eq!(out.len(), 1);
        assert!(out[0].contains("conn id={} ok"), "{:?}", out[0]);
        assert!(out[0].starts_with("[×4]"));
    }

    #[test]
    fn below_threshold_is_untouched() {
        let input = v("worker 1 done\nworker 2 done");
        let out = apply(input.clone(), 3);
        assert_eq!(out, input);
    }
}
