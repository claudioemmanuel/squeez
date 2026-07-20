use crate::config::Config;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Intensity {
    Lite,
    Full,
    Ultra,
}

impl Intensity {
    pub fn as_str(self) -> &'static str {
        match self {
            Intensity::Lite => "Lite",
            Intensity::Full => "Full",
            Intensity::Ultra => "Ultra",
        }
    }
}

/// Token budget for intensity/burn-rate math.
///
/// When `context_window_tokens` is configured (> 0), the budget IS the real
/// host window — adaptive intensity then keys off measured context against
/// the actual window (e.g. 200000, or 1000000 for a `[1m]` model). Otherwise
/// falls back to the legacy `compact_threshold_tokens * 5 / 4` formula.
pub fn budget(cfg: &Config) -> u64 {
    budget_for(cfg, 0)
}

/// Budget keyed to the real host window. Precedence: explicit
/// `context_window_tokens` config > `real_ctx_window` detected from the
/// transcript model id > legacy `compact_threshold_tokens * 5/4`. This is what
/// stops squeez warning against the wrong window (e.g. treating 17%-of-1M as
/// "critical" because it assumed the 112.5K legacy budget).
pub fn budget_for(cfg: &Config, real_ctx_window: u64) -> u64 {
    if cfg.context_window_tokens > 0 {
        return cfg.context_window_tokens;
    }
    if real_ctx_window > 0 {
        return real_ctx_window;
    }
    // `compact_threshold_tokens == 0` is the explicit "misconfigured budget"
    // sentinel → keep the always-Ultra safety fallback.
    if cfg.compact_threshold_tokens == 0 {
        return 0;
    }
    // Nothing pinned and nothing observed yet: floor the window at the modern
    // Claude minimum (200K) instead of the legacy compact-threshold formula,
    // which yielded ~112K for the default 90K threshold and made squeez warn
    // "critical context" and over-compress on 200K/1M-window models before any
    // transcript signal arrived. The real window still wins the moment it's
    // detected; an explicit context_window_tokens still overrides everything.
    cfg.compact_threshold_tokens
        .saturating_mul(5)
        .saturating_div(4)
        .max(crate::context::transcript::STANDARD_WINDOW)
}

/// Legacy constants kept for any callers that imported them by name before phase 5.
/// The actual default trigger is 65% (`ultra_trigger_pct: 0.65` in Config).
/// These values (80/100) were the original hardcoded threshold; prefer `cfg.ultra_trigger_pct`.
pub const ULTRA_TRIGGER_NUM: u64 = 80;
pub const ULTRA_TRIGGER_DEN: u64 = 100;

/// Derive intensity from config + current usage.
///
/// When `adaptive_intensity = false` the system uses Lite (no scaling at all).
///
/// When `adaptive_intensity = true` (default), the system actually adapts to
/// session pressure rather than always sitting at maximum aggression:
///
/// * `used < ultra_trigger_pct of budget` → Full (×0.6 — gentle compression)
/// * `used ≥ ultra_trigger_pct of budget` → Ultra (×0.3 — emergency compression)
///
/// The threshold is configurable via `ultra_trigger_pct` (default 0.65).
pub fn derive(used: u64, cfg: &Config) -> Intensity {
    derive_with(used, cfg, 0)
}

/// Like [`derive`] but keyed to the real host window (`real_ctx_window`, 0 =
/// unknown → legacy budget).
pub fn derive_with(used: u64, cfg: &Config, real_ctx_window: u64) -> Intensity {
    if !cfg.adaptive_intensity {
        return Intensity::Lite;
    }
    let b = budget_for(cfg, real_ctx_window);
    if b == 0 {
        // Misconfigured budget — fall back to the previous always-Ultra behavior.
        return Intensity::Ultra;
    }
    // Scale pct to a 10000-based integer to avoid f32/f64 precision issues
    // (e.g. 0.80f32 as f64 = 0.8000000119..., causing 80%-exactly boundary
    // to compare as < 80% when using floating-point).  Integer comparison:
    //   used * 10000 >= b * pct_10000
    let pct_10000 = (cfg.ultra_trigger_pct.clamp(0.0, 1.0) * 10_000.0).round() as u64;
    if used.saturating_mul(10_000) >= b.saturating_mul(pct_10000) {
        Intensity::Ultra
    } else {
        Intensity::Full
    }
}

/// Return a clone of `cfg` with line/dedup limits scaled by `level`.
/// Floors enforced so we never reduce to zero.
pub fn scale(cfg: &Config, level: Intensity) -> Config {
    let mut c = cfg.clone();
    let (lines_mult_num, lines_mult_den, dedup_floor) = match level {
        Intensity::Lite => return c,
        Intensity::Full => (6u64, 10u64, 2usize),  // ×0.6
        Intensity::Ultra => (3u64, 10u64, 2usize), // ×0.3
    };
    c.max_lines = scale_usize(c.max_lines, lines_mult_num, lines_mult_den, 20);
    c.git_log_max_commits = scale_usize(c.git_log_max_commits, lines_mult_num, lines_mult_den, 5);
    c.git_diff_max_lines = scale_usize(c.git_diff_max_lines, lines_mult_num, lines_mult_den, 20);
    c.docker_logs_max_lines =
        scale_usize(c.docker_logs_max_lines, lines_mult_num, lines_mult_den, 20);
    c.find_max_results = scale_usize(c.find_max_results, lines_mult_num, lines_mult_den, 10);
    c.summarize_threshold_lines = scale_usize(
        c.summarize_threshold_lines,
        lines_mult_num,
        lines_mult_den,
        50,
    );

    // dedup_min: Full ×0.66 → ceil to 2; Ultra ×0.5 → ceil to 2
    let dedup_num = match level {
        Intensity::Full => 66u64,
        Intensity::Ultra => 50u64,
        Intensity::Lite => 100u64,
    };
    c.dedup_min = scale_usize(c.dedup_min, dedup_num, 100, dedup_floor);
    c
}

fn scale_usize(v: usize, num: u64, den: u64, floor: usize) -> usize {
    let scaled = (v as u64).saturating_mul(num) / den.max(1);
    (scaled as usize).max(floor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config::default()
    }

    #[test]
    fn adaptive_enabled_at_zero_is_full() {
        // Empty session: gentler compression.
        assert_eq!(derive(0, &cfg()), Intensity::Full);
    }

    #[test]
    fn adaptive_enabled_just_below_threshold_is_full() {
        let c = cfg();
        // 60% of budget — still Full (threshold is now 65%)
        let used = budget(&c) * 60 / 100;
        assert_eq!(derive(used, &c), Intensity::Full);
    }

    #[test]
    fn adaptive_enabled_at_threshold_is_ultra() {
        let c = cfg();
        // 80% of budget — past the 65% default threshold → Ultra
        let used = budget(&c) * 80 / 100;
        assert_eq!(derive(used, &c), Intensity::Ultra);
    }

    #[test]
    fn adaptive_enabled_at_full_budget_is_ultra() {
        let c = cfg();
        assert_eq!(derive(budget(&c), &c), Intensity::Ultra);
    }

    #[test]
    fn adaptive_enabled_above_budget_is_ultra() {
        let c = cfg();
        assert_eq!(derive(budget(&c) * 5, &c), Intensity::Ultra);
    }

    #[test]
    fn adaptive_disabled_always_lite() {
        let mut c = cfg();
        c.adaptive_intensity = false;
        assert_eq!(derive(0, &c), Intensity::Lite);
        assert_eq!(derive(budget(&c) * 5, &c), Intensity::Lite);
    }

    #[test]
    fn window_override_replaces_budget_formula() {
        let mut c = cfg();
        c.context_window_tokens = 1_000_000;
        assert_eq!(budget(&c), 1_000_000);
        // 286K used on a 1M window: well below the 65% trigger → Full.
        assert_eq!(derive(286_000, &c), Intensity::Full);
        // 700K used (70%) → Ultra.
        assert_eq!(derive(700_000, &c), Intensity::Ultra);
    }

    #[test]
    fn real_context_beyond_legacy_budget_is_ultra() {
        // Audit CF-1 regression: 286K measured context vs the default 112.5K
        // budget must escalate to Ultra (the audited session never did,
        // because squeez only saw its own ~5.7K of bash bytes).
        let c = cfg();
        assert_eq!(derive(286_000, &c), Intensity::Ultra);
    }

    #[test]
    fn zero_budget_falls_back_to_ultra() {
        let mut c = cfg();
        c.compact_threshold_tokens = 0;
        // Misconfigured (budget=0) — old behavior preserved.
        assert_eq!(derive(0, &c), Intensity::Ultra);
        assert_eq!(derive(1000, &c), Intensity::Ultra);
    }

    #[test]
    fn default_budget_floors_at_modern_window() {
        // With nothing pinned/observed, the fallback budget is the modern 200K
        // window minimum, not the legacy ~112K (which fired false "critical
        // context" warnings on 200K/1M models). Default compact_threshold is
        // 90K → legacy formula 112.5K → floored to 200K.
        let mut c = cfg();
        c.context_window_tokens = 0;
        c.compact_threshold_tokens = 90_000;
        assert_eq!(budget(&c), crate::context::transcript::STANDARD_WINDOW);
        // 100K used is 50% of 200K → still Full, not the old always-critical.
        assert_eq!(derive(100_000, &c), Intensity::Full);
    }

    #[test]
    fn scale_lite_is_passthrough() {
        let c = cfg();
        let s = scale(&c, Intensity::Lite);
        assert_eq!(s.max_lines, c.max_lines);
        assert_eq!(s.dedup_min, c.dedup_min);
    }

    #[test]
    fn scale_full_shrinks() {
        let c = cfg();
        let s = scale(&c, Intensity::Full);
        assert!(s.max_lines < c.max_lines);
        assert!(s.git_diff_max_lines < c.git_diff_max_lines);
    }

    #[test]
    fn scale_ultra_shrinks_more_than_full() {
        let c = cfg();
        let f = scale(&c, Intensity::Full);
        let u = scale(&c, Intensity::Ultra);
        assert!(u.max_lines <= f.max_lines);
        assert!(u.git_diff_max_lines <= f.git_diff_max_lines);
    }

    #[test]
    fn floors_enforced() {
        let mut c = cfg();
        c.max_lines = 10;
        c.git_diff_max_lines = 5;
        c.dedup_min = 1;
        let s = scale(&c, Intensity::Ultra);
        assert!(s.max_lines >= 20, "max_lines floor: got {}", s.max_lines);
        assert!(s.git_diff_max_lines >= 20);
        assert!(s.dedup_min >= 2);
    }
}
