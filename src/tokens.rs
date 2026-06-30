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
}
