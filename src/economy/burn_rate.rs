use crate::config::Config;
use crate::context::cache::SessionContext;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Minimum entries in burn window before predictions are reliable.
const MIN_WINDOW_FOR_PREDICTION: usize = 3;

// ── Prediction ────────────────────────────────────────────────────────────────

/// Estimate calls remaining before the context budget is exhausted.
/// Returns `None` if the burn window has fewer than `MIN_WINDOW_FOR_PREDICTION`
/// entries (not enough data for a reliable estimate).
///
/// Budget = compact_threshold_tokens * 5 / 4 (same as intensity.rs).
pub fn calls_remaining(ctx: &SessionContext, cfg: &Config) -> Option<u64> {
    if ctx.burn_window.len() < MIN_WINDOW_FOR_PREDICTION {
        return None;
    }
    // Median per-call cost rather than mean: a window mixing 0-token calls
    // with one large output makes the mean estimate swing wildly between
    // invocations (transcript audit CF-2 observed 688 → 2245 → 2807 within
    // minutes). The median is stable against those outliers.
    let mut sizes: Vec<u64> = ctx.burn_window.iter().map(|e| e.tokens).collect();
    sizes.sort_unstable();
    let per_call = sizes[sizes.len() / 2];
    if per_call == 0 {
        // Mostly-empty window — no reliable estimate (mirrors the old avg==0 guard).
        return None;
    }
    let budget = crate::context::intensity::budget(cfg);
    // Real measured context (when observed) is authoritative over squeez's
    // own byte counters, which miss MCP/image traffic entirely (CF-1).
    let counted = ctx.tokens_bash + ctx.tokens_read + ctx.tokens_grep + ctx.tokens_other;
    let used = counted.max(ctx.real_ctx_tokens);
    if used >= budget {
        return Some(0);
    }
    Some((budget - used) / per_call)
}

/// Returns a warning string when calls_remaining drops below the configured
/// threshold (`burn_rate_warn_calls`).
pub fn pressure_warning(ctx: &SessionContext, cfg: &Config) -> Option<String> {
    let remaining = calls_remaining(ctx, cfg)?;
    if remaining < cfg.burn_rate_warn_calls {
        Some(format_pressure_header(remaining))
    } else {
        None
    }
}

/// Format the budget pressure header segment.
pub fn format_pressure_header(remaining: u64) -> String {
    format!("[budget: ~{} calls left]", remaining)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::cache::BurnEntry;

    #[test]
    fn returns_none_with_few_entries() {
        let mut ctx = SessionContext::default();
        let cfg = Config::default();
        // Only 2 entries — below minimum
        ctx.burn_window.push(BurnEntry { call_n: 1, tokens: 100, ts: 0 });
        ctx.burn_window.push(BurnEntry { call_n: 2, tokens: 100, ts: 0 });
        assert!(calls_remaining(&ctx, &cfg).is_none());
    }

    #[test]
    fn predicts_with_three_entries() {
        let mut ctx = SessionContext::default();
        let cfg = Config::default();
        // 3 entries, avg = 1000 tokens/call
        for i in 1..=3 {
            ctx.burn_window.push(BurnEntry { call_n: i, tokens: 1000, ts: 0 });
        }
        // Budget = 112_500 (90_000 * 5/4), used = 0 → remaining = 112_500 / 1000 = 112
        let remaining = calls_remaining(&ctx, &cfg).unwrap();
        assert_eq!(remaining, 112);
    }

    #[test]
    fn accounts_for_used_tokens() {
        let mut ctx = SessionContext::default();
        let cfg = Config::default();
        ctx.tokens_bash = 100_000;
        for i in 1..=3 {
            ctx.burn_window.push(BurnEntry { call_n: i, tokens: 1000, ts: 0 });
        }
        // Budget = 112_500, used = 100_000 → remaining = 12_500 / 1000 = 12
        let remaining = calls_remaining(&ctx, &cfg).unwrap();
        assert_eq!(remaining, 12);
    }

    #[test]
    fn returns_zero_when_budget_exhausted() {
        let mut ctx = SessionContext::default();
        let cfg = Config::default();
        ctx.tokens_bash = 200_000; // over budget
        for i in 1..=3 {
            ctx.burn_window.push(BurnEntry { call_n: i, tokens: 1000, ts: 0 });
        }
        assert_eq!(calls_remaining(&ctx, &cfg).unwrap(), 0);
    }

    #[test]
    fn pressure_warning_when_low() {
        let mut ctx = SessionContext::default();
        let cfg = Config::default(); // burn_rate_warn_calls = 20
        ctx.tokens_bash = 102_500; // near budget
        for i in 1..=3 {
            ctx.burn_window.push(BurnEntry { call_n: i, tokens: 1000, ts: 0 });
        }
        // remaining = (112_500 - 102_500) / 1000 = 10 < 20
        let warn = pressure_warning(&ctx, &cfg);
        assert!(warn.is_some());
        assert!(warn.unwrap().contains("~10 calls left"));
    }

    #[test]
    fn no_warning_when_plenty_of_budget() {
        let mut ctx = SessionContext::default();
        let cfg = Config::default();
        for i in 1..=3 {
            ctx.burn_window.push(BurnEntry { call_n: i, tokens: 1000, ts: 0 });
        }
        // remaining = 150 > 20
        assert!(pressure_warning(&ctx, &cfg).is_none());
    }

    #[test]
    fn format_header() {
        assert_eq!(format_pressure_header(42), "[budget: ~42 calls left]");
    }

    #[test]
    fn median_is_stable_against_outliers() {
        // Audit CF-2 regression: a window of tiny calls plus one huge output
        // made the mean-based estimate swing wildly (688 → 2245 → 2807).
        // Median per-call cost must not move when one outlier joins.
        let mut ctx = SessionContext::default();
        let cfg = Config::default();
        for i in 1..=5 {
            ctx.burn_window.push(BurnEntry { call_n: i, tokens: 100, ts: 0 });
        }
        let before = calls_remaining(&ctx, &cfg).unwrap();
        ctx.burn_window.push(BurnEntry { call_n: 6, tokens: 50_000, ts: 0 });
        let after = calls_remaining(&ctx, &cfg).unwrap();
        // Median stays 100 → estimate unchanged (mean would have ~89×'d the cost).
        assert_eq!(before, after);
    }

    #[test]
    fn real_context_overrides_squeez_counters() {
        // CF-1: measured context (286K) beyond budget → 0 calls left, even
        // when squeez's own counters saw almost nothing.
        let mut ctx = SessionContext::default();
        let cfg = Config::default(); // budget 112_500
        ctx.tokens_bash = 5_000;
        ctx.real_ctx_tokens = 286_000;
        for i in 1..=3 {
            ctx.burn_window.push(BurnEntry { call_n: i, tokens: 1000, ts: 0 });
        }
        assert_eq!(calls_remaining(&ctx, &cfg).unwrap(), 0);
    }

    #[test]
    fn grep_tokens_count_toward_used() {
        let mut ctx = SessionContext::default();
        let cfg = Config::default();
        ctx.tokens_grep = 112_500; // entire budget consumed via Grep
        for i in 1..=3 {
            ctx.burn_window.push(BurnEntry { call_n: i, tokens: 1000, ts: 0 });
        }
        assert_eq!(calls_remaining(&ctx, &cfg).unwrap(), 0);
    }

    #[test]
    fn all_zero_window_returns_none() {
        let mut ctx = SessionContext::default();
        let cfg = Config::default();
        for i in 1..=5 {
            ctx.burn_window.push(BurnEntry { call_n: i, tokens: 0, ts: 0 });
        }
        assert!(calls_remaining(&ctx, &cfg).is_none());
    }
}
