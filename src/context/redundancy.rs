use crate::context::cache::{CallEntry, SessionContext};
use crate::context::hash::fnv1a_64;

/// Minimum number of compressed lines for an output to be eligible for
/// redundancy lookup. Below this we don't dedup — collapses too much signal.
const MIN_LINES: usize = 2;

#[derive(Debug, Clone)]
pub struct RedundancyHit {
    pub short_hash: String,
    pub call_n: u64,
}

/// Compute the canonical hash + length for a compressed output.
fn fingerprint(output: &[String]) -> (u64, usize) {
    let joined = output.join("\n");
    (fnv1a_64(joined.as_bytes()), joined.len())
}

/// If `output` matches a recent call's hash AND length, return a hit.
pub fn check(ctx: &SessionContext, output: &[String]) -> Option<RedundancyHit> {
    if output.len() < MIN_LINES {
        return None;
    }
    let (h, len) = fingerprint(output);
    let entry: &CallEntry = ctx.lookup_recent(h, len)?;
    Some(RedundancyHit {
        short_hash: entry.short_hash.clone(),
        call_n: entry.call_n,
    })
}

/// Record this call's output for future redundancy lookups.
/// Returns the assigned call_n.
pub fn record(ctx: &mut SessionContext, cmd: &str, output: &[String]) -> u64 {
    let (h, len) = fingerprint(output);
    let call_n = ctx.next_call_n();
    ctx.record_call(cmd, h, len, call_n);
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
