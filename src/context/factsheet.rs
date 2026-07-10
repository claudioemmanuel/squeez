//! Factsheet (R1): deterministic, budget-capped extraction of
//! precision-critical identifiers from text that is about to be dropped by
//! the summarizer. When summarize.rs collapses a large output, everything
//! before the preserved tail vanishes — including exact tokens the model may
//! need verbatim later (git SHAs, UUIDs, versions, ticket codes, big
//! numbers). `extract` scans the dropped region and returns a small, stable
//! list of such identifiers so the summary can carry them forward.
//!
//! Zero-dependency: hand-rolled char scanners, no regex.

use std::collections::HashSet;

/// Maximum number of facts returned by `extract`.
pub const MAX_FACTS: usize = 16;
/// Total budget for the facts when joined with single separators (~64 tokens).
const MAX_JOINED_LEN: usize = 256;
/// Only the first 256 KiB of input are scanned — identifiers that matter
/// overwhelmingly appear early, and this bounds worst-case cost.
const SCAN_CAP_BYTES: usize = 256 * 1024;
/// Candidate token length bounds (after trimming surrounding punctuation).
const MIN_TOKEN_LEN: usize = 3;
const MAX_TOKEN_LEN: usize = 120;
/// Hard cap on unique candidates collected before ranking — bounds the
/// O(n²) substring-collapse pass on pathological inputs.
const MAX_CANDIDATES: usize = 512;

/// Characters that terminate a candidate token. `-` and `.` are NOT
/// delimiters — UUIDs, ticket codes, and versions contain them.
fn is_delim(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '"' | '\'' | '`' | '='
                | ':'
        )
}

/// Punctuation trimmed from both ends of a raw token (sentence-final dots,
/// bullet markers, issue-number hashes). None of the identifier classes can
/// legitimately start or end with these.
fn is_trim(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | '*' | '#' | '~' | '|' | '/' | '\\' | '-' | '+')
}

/// UUID: 8-4-4-4-12 hex groups separated by dashes (case-insensitive hex).
fn is_uuid(tok: &str) -> bool {
    let b = tok.as_bytes();
    if b.len() != 36 {
        return false;
    }
    for (i, &c) in b.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if c != b'-' {
                    return false;
                }
            }
            _ => {
                if !c.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }
    true
}

/// Hex / git-SHA token: 7–40 lowercase hex chars with at least one digit.
/// The digit requirement rejects English words that happen to be all-hex
/// letters ("deadbeef", "cafebabe" pass hex but carry no digit... they do
/// carry none — exactly the false positives this rule exists to drop).
fn is_hex_id(tok: &str) -> bool {
    let len = tok.len();
    if !(7..=40).contains(&len) {
        return false;
    }
    let mut has_digit = false;
    for c in tok.chars() {
        match c {
            '0'..='9' => has_digit = true,
            'a'..='f' => {}
            _ => return false,
        }
    }
    has_digit
}

/// Ticket code: 2–10 uppercase ASCII letters, a dash, then 1+ digits
/// (e.g. PROJ-1482, AB-123).
fn is_ticket(tok: &str) -> bool {
    let Some(dash) = tok.find('-') else {
        return false;
    };
    let (prefix, rest) = (&tok[..dash], &tok[dash + 1..]);
    (2..=10).contains(&prefix.len())
        && prefix.chars().all(|c| c.is_ascii_uppercase())
        && !rest.is_empty()
        && rest.chars().all(|c| c.is_ascii_digit())
}

/// Version string: optional leading `v`/`V`, then 2+ dot-separated numeric
/// components (e.g. 1.34.6, v2.0.1). One component ("v2") is too ambiguous.
fn is_version(tok: &str) -> bool {
    let body = tok.strip_prefix('v').or_else(|| tok.strip_prefix('V')).unwrap_or(tok);
    let mut components = 0usize;
    for part in body.split('.') {
        if part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        components += 1;
    }
    components >= 2
}

/// Classify a trimmed token into a priority tier, or None if it is not a
/// precision-critical identifier. Tiers: 0 = opaque ids (hex/UUID),
/// 1 = ticket codes, 2 = versions / large integers.
fn classify(tok: &str) -> Option<u8> {
    if is_uuid(tok) {
        return Some(0);
    }
    // All-digit tokens are large integers, never hex ids — check first so a
    // 7-digit number lands in tier 2 rather than masquerading as a SHA.
    if tok.chars().all(|c| c.is_ascii_digit()) {
        return if tok.len() >= 6 { Some(2) } else { None };
    }
    if is_hex_id(tok) {
        return Some(0);
    }
    if is_ticket(tok) {
        return Some(1);
    }
    if is_version(tok) {
        return Some(2);
    }
    None
}

/// Extract a deterministic, budget-capped list of precision-critical
/// identifiers from `text`. Output order: (tier asc, length desc, lex asc).
/// Exact duplicates are dropped; a candidate that is a substring of another
/// kept candidate is dropped (the most specific token wins). Stops when
/// either MAX_FACTS or the 256-char joined budget would be exceeded.
pub fn extract(text: &str) -> Vec<String> {
    // Cap the scan at 256 KiB, backing off to a char boundary.
    let scan = if text.len() > SCAN_CAP_BYTES {
        let mut end = SCAN_CAP_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        &text[..end]
    } else {
        text
    };

    // Pass 1: tokenize, trim, classify, dedup exact duplicates.
    let mut seen: HashSet<&str> = HashSet::new();
    let mut cands: Vec<(u8, &str)> = Vec::new();
    'scan: for raw in scan.split(is_delim) {
        let tok = raw.trim_matches(is_trim);
        if tok.len() < MIN_TOKEN_LEN || tok.len() > MAX_TOKEN_LEN {
            continue;
        }
        if let Some(tier) = classify(tok) {
            if seen.insert(tok) {
                cands.push((tier, tok));
                if cands.len() >= MAX_CANDIDATES {
                    break 'scan;
                }
            }
        }
    }

    // High-cardinality guard: versions / large integers that appear in bulk
    // are almost always generated sequences (per-crate versions in a build
    // log, loop counters), not precision-critical identifiers. If tier 2
    // alone yields more distinct candidates than the whole factsheet can
    // carry, drop the tier rather than fill the budget with noise. Opaque
    // ids (hex/UUID) and ticket codes stay — those are worth carrying even
    // in bulk.
    if cands.iter().filter(|&&(t, _)| t == 2).count() > MAX_FACTS {
        cands.retain(|&(t, _)| t != 2);
    }

    // Pass 2: substring collapse. Visit longest-first (lex tiebreak keeps it
    // deterministic) and drop any candidate contained in one already kept.
    let mut order: Vec<usize> = (0..cands.len()).collect();
    order.sort_by(|&a, &b| {
        cands[b]
            .1
            .len()
            .cmp(&cands[a].1.len())
            .then_with(|| cands[a].1.cmp(cands[b].1))
    });
    let mut kept: Vec<(u8, &str)> = Vec::new();
    for i in order {
        let (tier, tok) = cands[i];
        if !kept.iter().any(|&(_, k)| k.contains(tok)) {
            kept.push((tier, tok));
        }
    }

    // Pass 3: deterministic final order, then budget.
    kept.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| b.1.len().cmp(&a.1.len()))
            .then_with(|| a.1.cmp(b.1))
    });

    let mut out: Vec<String> = Vec::new();
    let mut joined_len = 0usize;
    for (_, tok) in kept {
        if out.len() >= MAX_FACTS {
            break;
        }
        // Count as if joined with a single-char separator.
        let add = tok.len() + usize::from(!out.is_empty());
        if joined_len + add > MAX_JOINED_LEN {
            break;
        }
        joined_len += add;
        out.push(tok.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_git_sha() {
        let facts = extract("commit a1347daf9b2c41e0 pushed to main");
        assert_eq!(facts, vec!["a1347daf9b2c41e0".to_string()]);
    }

    #[test]
    fn extracts_uuid() {
        let facts = extract("request id 550e8400-e29b-41d4-a716-446655440000 failed");
        assert_eq!(facts, vec!["550e8400-e29b-41d4-a716-446655440000".to_string()]);
    }

    #[test]
    fn extracts_version_and_ticket_and_large_int() {
        let facts = extract("released v1.34.6 for PROJ-1482 build 20260710");
        assert!(facts.contains(&"v1.34.6".to_string()));
        assert!(facts.contains(&"PROJ-1482".to_string()));
        assert!(facts.contains(&"20260710".to_string()));
        // Tier order: ticket (1) before version/number (2).
        let ticket_pos = facts.iter().position(|f| f == "PROJ-1482").unwrap();
        let ver_pos = facts.iter().position(|f| f == "v1.34.6").unwrap();
        assert!(ticket_pos < ver_pos);
    }

    #[test]
    fn hex_requires_a_digit() {
        // "deadbeef" is all hex letters but has no digit → rejected.
        assert!(extract("value deadbeef here").is_empty());
        assert_eq!(extract("value deadbee7 here"), vec!["deadbee7".to_string()]);
    }

    #[test]
    fn dedups_exact_duplicates() {
        let facts = extract("sha a1347daf9 then a1347daf9 again a1347daf9");
        assert_eq!(facts, vec!["a1347daf9".to_string()]);
    }

    #[test]
    fn substring_collapse_keeps_most_specific() {
        // Short SHA is a prefix of the full SHA → only the full one survives.
        let facts = extract("short a1347daf9 full a1347daf9b2c41e0aa55");
        assert_eq!(facts, vec!["a1347daf9b2c41e0aa55".to_string()]);
    }

    #[test]
    fn deterministic_across_runs() {
        let input = "PROJ-1 v1.2.3 550e8400-e29b-41d4-a716-446655440000 \
                     a1347daf9 123456789 AB-123 v2.0.1";
        assert_eq!(extract(input), extract(input));
    }

    #[test]
    fn caps_at_max_facts() {
        // 30 distinct short hex ids (7 chars each fit well under the char
        // budget) → exactly MAX_FACTS survive.
        let input: String = (0..30).map(|i| format!("abc{:04} ", i)).collect();
        let facts = extract(&input);
        assert_eq!(facts.len(), MAX_FACTS);
    }

    #[test]
    fn caps_joined_length() {
        // Long ticket numbers: each fact is ~40 chars, so the 256-char joined
        // budget cuts off well before MAX_FACTS.
        let input: String = (0..16)
            .map(|i| format!("ABCDEF-{:034} ", i))
            .collect();
        let facts = extract(&input);
        assert!(facts.len() < MAX_FACTS);
        let joined = facts.join(" ");
        assert!(joined.len() <= 256, "joined {} chars", joined.len());
    }

    #[test]
    fn trims_surrounding_punctuation() {
        let facts = extract("see (v1.34.6). and [PROJ-99] plus \"a1347daf9\",");
        assert!(facts.contains(&"v1.34.6".to_string()));
        assert!(facts.contains(&"PROJ-99".to_string()));
        assert!(facts.contains(&"a1347daf9".to_string()));
    }

    #[test]
    fn all_digit_token_is_large_int_not_hex() {
        // 7 digits satisfies the hex shape but must land in tier 2, after a
        // real hex id in the final ordering.
        let facts = extract("1234567 and abc1234");
        assert_eq!(
            facts,
            vec!["abc1234".to_string(), "1234567".to_string()]
        );
    }

    #[test]
    fn rejects_small_numbers_and_plain_words() {
        // 5 digits < large-int floor; "v3" has one component; plain words and
        // small numbers never classify.
        assert!(extract("12345 words only here v3 line 42").is_empty());
    }

    #[test]
    fn bulk_versions_are_noise_but_opaque_ids_survive() {
        // >MAX_FACTS distinct versions (a build log pattern) → tier 2 is
        // dropped wholesale; a lone SHA in the same text still comes through.
        let mut input: String = (0..30).map(|i| format!("v0.1.{} ", i)).collect();
        input.push_str("commit a1347daf9");
        assert_eq!(extract(&input), vec!["a1347daf9".to_string()]);
        // At or under the cap, versions are kept.
        let few: String = (0..3).map(|i| format!("v0.1.{} ", i)).collect();
        assert_eq!(extract(&few).len(), 3);
    }

    #[test]
    fn empty_input_empty_vec() {
        assert!(extract("").is_empty());
        assert!(extract("   \n\t ").is_empty());
    }

    #[test]
    fn scan_capped_at_256kib() {
        // Identifier placed past the cap is not scanned.
        let mut big = " ".repeat(SCAN_CAP_BYTES + 10);
        big.push_str(" a1347daf9 ");
        assert!(extract(&big).is_empty());
    }
}
