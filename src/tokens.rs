//! Cheap, zero-dep token estimate (P5).
//!
//! squeez long used a flat `bytes / 4`. That's a decent rule of thumb for
//! English prose but wrong in two directions that matter for *when to
//! compress*:
//!   - code is punctuation-dense (`fn main() {}`, `a.b.c(d)`) → real
//!     tokenizers emit more than chars/4;
//!   - CJK / non-ASCII undercounts badly → chars/4 says ~1 token for 4 CJK
//!     chars, real tokenizers emit ~4-8.
//!
//! `estimate` classifies characters and sums:
//!   - ASCII alphanumerics pack ~3.8 chars/token (real Claude BPE runs a touch
//!     denser than the old chars/4 rule on prose),
//!   - ASCII symbols ~2 chars/token (often merged, but denser than words),
//!   - non-ASCII ~1 token/char (CJK/emoji),
//!   - whitespace is free (a token boundary).
//!
//! This is not a tokenizer — it ships no vocab and stays stdlib-only. It just
//! lands closer to real BPE on mixed agent output than a flat divide. The
//! benchmark suite keeps its fixed `chars/4` convention for reproducibility.
//!
//! `estimate_scaled` multiplies the base estimate by a per-model density
//! factor (`tokenizer_scale`, config-driven). Newer tokenizers (e.g. Sonnet 5)
//! emit ~1.0–1.35× more tokens for the same text; `scale = 1.0` is the legacy
//! path and leaves the estimate untouched.

/// Estimate the number of tokens in `s`.
pub fn estimate(s: &str) -> usize {
    let mut alnum = 0usize;
    let mut punct = 0usize;
    let mut wide = 0usize;
    for c in s.chars() {
        if c.is_ascii() {
            if c.is_ascii_alphanumeric() {
                alnum += 1;
            } else if !c.is_ascii_whitespace() {
                punct += 1;
            }
            // ASCII whitespace contributes nothing — it's a token boundary.
        } else {
            wide += 1;
        }
    }
    let est = (alnum as f64 / 3.8) + (punct as f64 / 2.0) + wide as f64;
    let est = est.ceil() as usize;
    if est == 0 && !s.is_empty() {
        1
    } else {
        est
    }
}

// ── Content-class calibrated density (R2) ──────────────────────────────────
//
// pxpipe measured real tokenizer density on production tool output:
// dense payloads (JSON/code/diffs, N=391) run ~1.91 chars/token, while
// English prose runs ~3.5-3.7 chars/token. A flat divisor is wrong in both
// directions, so `classify` fingerprints the payload coarsely and
// `estimate_classed` picks a calibrated divisor per class.

/// Content class of a payload, used to pick a calibrated chars/token density.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentClass {
    Dense,
    Prose,
    Mixed,
}

/// Bytes of `s` that `classify` inspects. Density is stable across a payload,
/// so the head is enough — no need to rescan a multi-MB capture.
const CLASSIFY_SAMPLE_BYTES: usize = 64 * 1024;
/// Punctuation/symbol share of non-whitespace chars at or above which content
/// is Dense. pxpipe's dense corpus (JSON/code/diffs, ~1.91 chars/token) runs
/// ≥ ~18% ASCII punctuation.
const DENSE_PUNCT_RATIO: f64 = 0.18;
/// Share at or below which content is Prose — English prose (~3.5-3.7
/// chars/token in the same calibration) rarely exceeds ~8% punctuation.
const PROSE_PUNCT_RATIO: f64 = 0.08;
/// Calibrated divisor for Dense: pxpipe's measured 1.91 rounded to a
/// conservative 2.0.
const DENSE_CHARS_PER_TOKEN: f64 = 2.0;
/// Calibrated divisor for Prose (pxpipe prose upper bound).
const PROSE_CHARS_PER_TOKEN: f64 = 3.7;

/// Coarsely classify `s` by tokenizer density: punctuation share plus a
/// line-shape check (JSON/diff/table lines start with `{ " + - |` or carry
/// `=>` / `::` / `==`).
pub fn classify(s: &str) -> ContentClass {
    let mut end = s.len().min(CLASSIFY_SAMPLE_BYTES);
    while end < s.len() && !s.is_char_boundary(end) {
        end += 1;
    }
    let sample = &s[..end];

    let mut punct = 0usize;
    let mut nonws = 0usize;
    for c in sample.chars() {
        if c.is_whitespace() {
            continue;
        }
        nonws += 1;
        if c.is_ascii() && !c.is_ascii_alphanumeric() {
            punct += 1;
        }
    }
    if nonws == 0 {
        return ContentClass::Mixed;
    }

    // Structural shape: majority of non-empty lines look like JSON/diff/table.
    let mut structural = 0usize;
    let mut nonempty = 0usize;
    for line in sample.lines() {
        let t = line.trim_start();
        if t.is_empty() {
            continue;
        }
        nonempty += 1;
        if matches!(t.as_bytes()[0], b'{' | b'"' | b'+' | b'-' | b'|')
            || t.contains("=>")
            || t.contains("::")
            || t.contains("==")
        {
            structural += 1;
        }
    }
    let looks_structured = structural * 2 > nonempty;

    let ratio = punct as f64 / nonws as f64;
    if ratio >= DENSE_PUNCT_RATIO || looks_structured {
        ContentClass::Dense
    } else if ratio <= PROSE_PUNCT_RATIO {
        ContentClass::Prose
    } else {
        ContentClass::Mixed
    }
}

/// Estimate tokens in `s` using the content-class calibrated densities:
/// Dense → chars/2.0, Prose → chars/3.7, Mixed → the char-class `estimate`.
/// `scale` applies like `estimate_scaled`; the non-empty floor of 1 holds.
pub fn estimate_classed(s: &str, scale: f32) -> usize {
    let base = match classify(s) {
        ContentClass::Dense => (s.chars().count() as f64 / DENSE_CHARS_PER_TOKEN).ceil() as usize,
        ContentClass::Prose => (s.chars().count() as f64 / PROSE_CHARS_PER_TOKEN).ceil() as usize,
        ContentClass::Mixed => estimate(s),
    };
    let scaled = ((base as f64) * scale as f64).ceil() as usize;
    if scaled == 0 && !s.is_empty() {
        1
    } else {
        scaled
    }
}

/// Estimate tokens in `s`, scaled by a model's tokenizer density factor.
///
/// `scale = 1.0` returns `estimate(s)` unchanged (legacy). Values > 1.0 model
/// denser tokenizers (e.g. ~1.15 for Sonnet 5). The non-empty floor of 1 is
/// preserved.
pub fn estimate_scaled(s: &str, scale: f32) -> usize {
    let scaled = ((estimate(s) as f64) * scale as f64).ceil() as usize;
    if scaled == 0 && !s.is_empty() {
        1
    } else {
        scaled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_zero() {
        assert_eq!(estimate(""), 0);
    }

    #[test]
    fn non_empty_is_at_least_one() {
        assert_eq!(estimate("a"), 1);
        assert_eq!(estimate("."), 1);
    }

    #[test]
    fn ascii_prose_is_near_chars_over_four() {
        // "the quick brown fox" — 16 alnum chars → ~4 tokens. chars/4 of the
        // 19-char string is ~4 too, so we stay in the same ballpark.
        let s = "the quick brown fox";
        let est = estimate(s);
        let naive = s.len() / 4;
        assert!(
            (est as i64 - naive as i64).abs() <= 2,
            "prose estimate {est} should track chars/4 {naive}"
        );
    }

    #[test]
    fn code_counts_more_than_naive() {
        // Punctuation-dense code: chars/4 undercounts the real token cost.
        let code = "a.b.c(d, e){return x==y;}";
        assert!(
            estimate(code) > code.len() / 4,
            "punctuation-dense code should exceed chars/4"
        );
    }

    #[test]
    fn cjk_counts_far_more_than_bytes_over_four() {
        // 4 CJK chars (12 UTF-8 bytes). bytes/4 = 3; real tokenizers ~4-8.
        let cjk = "你好世界";
        let est = estimate(cjk);
        assert!(est >= 4, "CJK should count ~1 token/char, got {est}");
        assert!(est > cjk.len() / 4, "must beat the bytes/4 undercount");
    }

    #[test]
    fn whitespace_is_free() {
        assert_eq!(estimate("ab cd"), estimate("ab  cd"));
    }

    #[test]
    fn scale_one_is_identity() {
        for s in ["", "hello world", "a.b.c(d)", "你好"] {
            assert_eq!(estimate_scaled(s, 1.0), estimate(s), "scale 1.0 != base for {s:?}");
        }
    }

    #[test]
    fn scale_above_one_counts_more() {
        let s = "the quick brown fox jumps over the lazy dog";
        assert!(
            estimate_scaled(s, 1.3) > estimate(s),
            "denser tokenizer scale should raise the estimate"
        );
    }

    #[test]
    fn scaled_empty_is_zero() {
        assert_eq!(estimate_scaled("", 1.3), 0);
    }

    #[test]
    fn json_classifies_dense_and_beats_chars_over_four() {
        let json = r#"{"name":"squeez","version":"1.34.6","deps":{"libc":"0.2"},"flags":[true,false]}"#;
        assert_eq!(classify(json), ContentClass::Dense);
        assert!(
            estimate_classed(json, 1.0) > json.chars().count() / 4,
            "dense JSON should exceed the chars/4 rule"
        );
    }

    #[test]
    fn english_prose_classifies_prose() {
        let prose = "the quick brown fox jumps over the lazy dog and then trots quietly home";
        assert_eq!(classify(prose), ContentClass::Prose);
    }

    #[test]
    fn mixed_agent_output_falls_back_to_estimate() {
        // Prose with sprinkled paths/versions: punct ratio between the Prose
        // and Dense thresholds, no structural line shape.
        let mixed = "Compiling squeez v1.34.6 (/Users/me/squeez)\nFinished dev profile in 2.41s";
        assert_eq!(classify(mixed), ContentClass::Mixed);
        assert_eq!(estimate_classed(mixed, 1.0), estimate(mixed));
    }

    #[test]
    fn classed_scale_above_one_counts_more() {
        let prose = "the quick brown fox jumps over the lazy dog and then trots quietly home";
        assert!(estimate_classed(prose, 1.3) > estimate_classed(prose, 1.0));
    }

    #[test]
    fn classed_empty_is_zero() {
        assert_eq!(estimate_classed("", 1.0), 0);
        assert_eq!(estimate_classed("", 1.3), 0);
    }
}
