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

/// Derive intensity from config + current usage.
/// When `adaptive_intensity` is enabled (the default), squeez compresses
/// at maximum aggression (Ultra) regardless of budget. The Lite/Full tiers
/// remain in the type so users can opt into a softer mode by disabling
/// adaptive_intensity (which falls back to Lite — no scaling).
///
/// `used` is accepted for forward compatibility but is currently unused.
pub fn derive(_used: u64, cfg: &Config) -> Intensity {
    if cfg.adaptive_intensity {
        Intensity::Ultra
    } else {
        Intensity::Lite
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
    fn adaptive_enabled_at_zero_is_ultra() {
        assert_eq!(derive(0, &cfg()), Intensity::Ultra);
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
