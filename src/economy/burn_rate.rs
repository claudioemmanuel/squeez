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
    let total: u64 = ctx.burn_window.iter().map(|e| e.tokens).sum();
    let avg = total / ctx.burn_window.len() as u64;
    if avg == 0 {
        return None;
    }
    let budget = cfg.compact_threshold_tokens * 5 / 4;
    let used = ctx.tokens_bash + ctx.tokens_read + ctx.tokens_other;
    if used >= budget {
        return Some(0);
    }
    Some((budget - used) / avg)
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
        // Budget = 150_000, used = 0 → remaining = 150_000 / 1000 = 150
        let remaining = calls_remaining(&ctx, &cfg).unwrap();
        assert_eq!(remaining, 150);
    }

    #[test]
    fn accounts_for_used_tokens() {
        let mut ctx = SessionContext::default();
        let cfg = Config::default();
        ctx.tokens_bash = 100_000;
        for i in 1..=3 {
            ctx.burn_window.push(BurnEntry { call_n: i, tokens: 1000, ts: 0 });
        }
        // Budget = 150_000, used = 100_000 → remaining = 50_000 / 1000 = 50
        let remaining = calls_remaining(&ctx, &cfg).unwrap();
        assert_eq!(remaining, 50);
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
        ctx.tokens_bash = 140_000; // near budget
        for i in 1..=3 {
            ctx.burn_window.push(BurnEntry { call_n: i, tokens: 1000, ts: 0 });
        }
        // remaining = (150_000 - 140_000) / 1000 = 10 < 20
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
}
