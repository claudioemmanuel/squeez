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

/// Budget = compact_threshold_tokens * 5 / 4 (matches existing wrap.rs math).
pub fn budget(cfg: &Config) -> u64 {
    cfg.compact_threshold_tokens.saturating_mul(5) / 4
}

/// Fraction of budget at which Full graduates to Ultra. Default 80%.
/// Overridable via `ultra_trigger_pct` in config.ini. Kept as pub constants
/// for any callers that imported them by name before phase 5.
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
    if !cfg.adaptive_intensity {
        return Intensity::Lite;
    }
    let b = budget(cfg);
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
        // Exactly 80% — graduates to Ultra
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
    fn zero_budget_falls_back_to_ultra() {
        let mut c = cfg();
        c.compact_threshold_tokens = 0;
        // Misconfigured (budget=0) — old behavior preserved.
        assert_eq!(derive(0, &c), Intensity::Ultra);
        assert_eq!(derive(1000, &c), Intensity::Ultra);
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
