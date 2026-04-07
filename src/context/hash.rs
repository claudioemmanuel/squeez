// FNV-1a 64-bit hash. Zero-dep, ~10 lines, good distribution for short strings.
// Reference: http://www.isthe.com/chongo/tech/comp/fnv/

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// First 8 hex chars of a 64-bit hash, lowercase.
pub fn short_hex(h: u64) -> String {
    format!("{:016x}", h).chars().take(8).collect()
}

/// Maximum number of shingle hashes kept per call. Bounds memory at ~2 KiB
/// per call (k * 8 bytes) and JSON serialization at ~6 KiB per call.
pub const SHINGLE_K: usize = 96;

/// Bottom-k MinHash sketch of the input text using whitespace-token trigrams.
///
/// Returns a sorted, deduplicated `Vec<u64>` of at most `SHINGLE_K` smallest
/// FNV-1a hashes of consecutive 3-token shingles. Whitespace is normalized
/// (collapsed runs) so that "a  b  c" and "a b c" yield identical sketches.
///
/// The bottom-k subset of two sets approximates Jaccard well when k is much
/// smaller than the universe size, which holds for typical command output.
pub fn shingle_minhash(text: &str) -> Vec<u64> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    if tokens.len() < 3 {
        return Vec::new();
    }
    let mut hashes: Vec<u64> = Vec::with_capacity(tokens.len().saturating_sub(2));
    for w in tokens.windows(3) {
        // Build a trigram with single-space joiner; w has length 3.
        let mut buf = String::with_capacity(w[0].len() + w[1].len() + w[2].len() + 2);
        buf.push_str(w[0]);
        buf.push(' ');
        buf.push_str(w[1]);
        buf.push(' ');
        buf.push_str(w[2]);
        hashes.push(fnv1a_64(buf.as_bytes()));
    }
    hashes.sort_unstable();
    hashes.dedup();
    if hashes.len() > SHINGLE_K {
        hashes.truncate(SHINGLE_K);
    }
    hashes
}

/// Jaccard similarity of two **sorted, deduplicated** `u64` slices,
/// computed in O(|a| + |b|). Returns 0.0 if either side is empty.
pub fn jaccard(a: &[u64], b: &[u64]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut i = 0usize;
    let mut j = 0usize;
    let mut intersection = 0usize;
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Equal => {
                intersection += 1;
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
        }
    }
    let union = a.len() + b.len() - intersection;
    if union == 0 {
        0.0
    } else {
        intersection as f32 / union as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_is_offset_basis() {
        assert_eq!(fnv1a_64(b""), FNV_OFFSET);
    }

    #[test]
    fn known_vector() {
        // FNV-1a("a") = 0xaf63dc4c8601ec8c
        assert_eq!(fnv1a_64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn different_inputs_differ() {
        assert_ne!(fnv1a_64(b"foo"), fnv1a_64(b"bar"));
    }

    #[test]
    fn short_hex_length() {
        let h = fnv1a_64(b"squeez");
        assert_eq!(short_hex(h).len(), 8);
        assert!(short_hex(h).chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn shingle_minhash_short_text_is_empty() {
        // Fewer than 3 tokens → no trigram
        assert!(shingle_minhash("hi there").is_empty());
        assert!(shingle_minhash("").is_empty());
    }

    #[test]
    fn shingle_minhash_is_whitespace_invariant() {
        let a = shingle_minhash("the quick brown fox jumps over the lazy dog");
        let b = shingle_minhash("the   quick brown   fox jumps over\tthe lazy dog");
        let c = shingle_minhash("the\nquick\nbrown\nfox\njumps\nover\nthe\nlazy\ndog");
        assert_eq!(a, b, "multi-space difference must produce identical sketch");
        assert_eq!(a, c, "newline vs space must produce identical sketch");
    }

    #[test]
    fn shingle_minhash_is_sorted_and_capped() {
        let words: Vec<String> = (0..500).map(|i| format!("token{}", i)).collect();
        let text = words.join(" ");
        let s = shingle_minhash(&text);
        assert!(s.len() <= SHINGLE_K, "got {} shingles", s.len());
        assert!(s.windows(2).all(|w| w[0] < w[1]), "sketch must be sorted+dedup");
    }

    #[test]
    fn jaccard_identical_is_one() {
        let s = shingle_minhash("the quick brown fox jumps over the lazy dog");
        assert_eq!(jaccard(&s, &s), 1.0);
    }

    #[test]
    fn jaccard_disjoint_is_zero() {
        let a = shingle_minhash("apple banana cherry date elderberry fig");
        let b = shingle_minhash("alpha bravo charlie delta echo foxtrot");
        assert_eq!(jaccard(&a, &b), 0.0);
    }

    #[test]
    fn jaccard_one_token_changed_is_high() {
        // Only one trigram differs out of many → similarity should be high
        let words: Vec<String> = (0..30).map(|i| format!("word{}", i)).collect();
        let a = shingle_minhash(&words.join(" "));
        let mut w2 = words.clone();
        w2[15] = "DIFFERENT".to_string();
        let b = shingle_minhash(&w2.join(" "));
        let j = jaccard(&a, &b);
        // Three trigrams contain word15 (positions 13,14,15), so 3 out of ~28
        // unique trigrams differ → Jaccard ≈ 25/(25+3+3) ≈ 0.81
        assert!(j > 0.7, "expected high similarity, got {}", j);
        assert!(j < 1.0, "expected <1.0, got {}", j);
    }
}
