use crate::context::cache::SessionContext;
use crate::context::hash::{fnv1a_64, shingle_minhash};

/// Minimum number of compressed lines for an output to be eligible for
/// redundancy lookup. Below this we don't dedup — collapses too much signal.
const MIN_LINES: usize = 2;

/// Minimum number of lines for fuzzy (shingle-Jaccard) matching to engage.
/// Below this, only exact-hash matches are returned. Short outputs have too
/// few trigrams for Jaccard to discriminate reliably.
const MIN_LINES_FUZZY: usize = 6;

#[derive(Debug, Clone)]
pub struct RedundancyHit {
    pub short_hash: String,
    pub call_n: u64,
    /// `Some(j)` if this hit came from the fuzzy similarity branch
    /// (where `j` is the Jaccard score in [SIMILARITY_THRESHOLD, 1.0]),
    /// `None` if it was an exact hash+length match.
    pub similarity: Option<f32>,
}

/// Compute the canonical hash + length from a pre-joined string.
fn fingerprint_str(joined: &str) -> (u64, usize) {
    (fnv1a_64(joined.as_bytes()), joined.len())
}

/// If `output` matches a recent call, return a hit. The fast path is exact
/// hash + length match (existing behavior). On miss, the fuzzy path computes
/// a MinHash sketch of `output` and checks for a Jaccard ≥ SIMILARITY_THRESHOLD
/// match against any recent call within a length-ratio guard.
pub fn check(ctx: &SessionContext, output: &[String]) -> Option<RedundancyHit> {
    if output.len() < MIN_LINES {
        return None;
    }
    let joined = output.join("\n");
    let (h, len) = fingerprint_str(&joined);
    // Fast path: exact hash + length match.
    if let Some(entry) = ctx.lookup_recent(h, len) {
        return Some(RedundancyHit {
            short_hash: entry.short_hash.clone(),
            call_n: entry.call_n,
            similarity: None,
        });
    }
    // Fuzzy path: shingle-Jaccard against recent calls. Skip for short outputs
    // where trigram count is too low to discriminate.
    if output.len() < MIN_LINES_FUZZY {
        return None;
    }
    let shingles = shingle_minhash(&joined);
    if shingles.is_empty() {
        return None;
    }
    let m = ctx.lookup_similar(&shingles, len)?;
    Some(RedundancyHit {
        short_hash: m.short_hash,
        call_n: m.call_n,
        similarity: Some(m.similarity),
    })
}

/// Record this call's output for future redundancy lookups.
/// Returns the assigned call_n. Stores both the exact hash and the MinHash
/// sketch so that future calls can match either exactly or fuzzily.
pub fn record(ctx: &mut SessionContext, cmd: &str, output: &[String]) -> u64 {
    let joined = output.join("\n");
    let (h, len) = fingerprint_str(&joined);
    let call_n = ctx.next_call_n();
    let shingles = if output.len() >= MIN_LINES_FUZZY {
        shingle_minhash(&joined)
    } else {
        Vec::new()
    };
    ctx.record_call_with_shingles(cmd, h, len, call_n, shingles);
    call_n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("line {}", i)).collect()
    }

    #[test]
    fn tiny_output_never_matches() {
        let mut ctx = SessionContext::default();
        let out = lines(1); // 1 < MIN_LINES=2
        record(&mut ctx, "cmd", &out);
        assert!(check(&ctx, &out).is_none(), "should not match tiny output");
    }

    #[test]
    fn exact_repeat_hits() {
        let mut ctx = SessionContext::default();
        let out = lines(10);
        record(&mut ctx, "git status", &out);
        let hit = check(&ctx, &out);
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().call_n, 1);
    }

    #[test]
    fn diff_by_one_line_misses() {
        let mut ctx = SessionContext::default();
        let mut out = lines(10);
        record(&mut ctx, "ls", &out);
        out[0] = "different".to_string();
        assert!(check(&ctx, &out).is_none());
    }

    #[test]
    fn outside_recent_window_misses() {
        let mut ctx = SessionContext::default();
        let target = lines(10);
        record(&mut ctx, "first", &target);
        // Push 17 more calls past the window (RECENT_WINDOW=16)
        for i in 0..17 {
            let other = (0..10).map(|j| format!("o{}-{}", i, j)).collect::<Vec<_>>();
            record(&mut ctx, &format!("c{}", i), &other);
        }
        assert!(
            check(&ctx, &target).is_none(),
            "first call should be outside RECENT_WINDOW"
        );
    }

    #[test]
    fn record_returns_incrementing_call_n() {
        let mut ctx = SessionContext::default();
        assert_eq!(record(&mut ctx, "a", &lines(10)), 1);
        assert_eq!(record(&mut ctx, "b", &lines(10)), 2);
    }
}
