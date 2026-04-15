/// Session efficiency scoring — computed at session finalization.
///
/// All scores are stored as basis points (0–10000) to avoid floating-point
/// serialization in the hand-rolled JSON. 10000 bp = 100%.

// ── Score struct ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct EfficiencyScore {
    /// How much output was compressed: (in - out) / in.
    pub compression_ratio_bp: u64,
    /// Tool choice quality: 1.0 - (agent_tokens / total_tokens).
    pub tool_choice_efficiency_bp: u64,
    /// Context reuse: dedup_hits / total_calls.
    pub context_reuse_rate_bp: u64,
    /// Budget utilization: total_tokens / budget (lower = better conservation).
    pub budget_utilization_bp: u64,
    /// Weighted average: compression 30%, tool_choice 30%, reuse 20%, budget 20%.
    pub overall_bp: u64,
}

// ── Computation ───────────────────────────────────────────────────────────────

/// Compute efficiency scores from raw session metrics.
///
/// - `total_in` / `total_out`: raw vs compressed token counts
/// - `agent_estimated_tokens`: cumulative sub-agent overhead estimate
/// - `total_tokens`: total tokens consumed in session
/// - `dedup_hits`: exact + fuzzy redundancy hits
/// - `total_calls`: total tool calls in session
/// - `budget`: context budget (compact_threshold * 5 / 4)
pub fn compute(
    total_in: u64,
    total_out: u64,
    agent_estimated_tokens: u64,
    total_tokens: u64,
    dedup_hits: u32,
    total_calls: u64,
    budget: u64,
) -> EfficiencyScore {
    let compression_ratio_bp = if total_in > 0 {
        ((total_in.saturating_sub(total_out)) * 10000 / total_in).min(10000)
    } else {
        0
    };

    let tool_choice_efficiency_bp = if total_tokens > 0 {
        let agent_ratio = (agent_estimated_tokens * 10000 / total_tokens).min(10000);
        10000u64.saturating_sub(agent_ratio)
    } else {
        10000
    };

    let context_reuse_rate_bp = if total_calls > 0 {
        ((dedup_hits as u64) * 10000 / total_calls).min(10000)
    } else {
        0
    };

    // Budget utilization — inverted so higher = better (less budget consumed).
    let budget_utilization_bp = if budget > 0 {
        let used_pct = (total_tokens * 10000 / budget).min(10000);
        10000u64.saturating_sub(used_pct)
    } else {
        0
    };

    // Weighted average: compression 30%, tool_choice 30%, reuse 20%, budget 20%.
    let overall_bp = (compression_ratio_bp * 30
        + tool_choice_efficiency_bp * 30
        + context_reuse_rate_bp * 20
        + budget_utilization_bp * 20)
        / 100;

    EfficiencyScore {
        compression_ratio_bp,
        tool_choice_efficiency_bp,
        context_reuse_rate_bp,
        budget_utilization_bp,
        overall_bp,
    }
}

/// Format efficiency score for MCP tool response.
pub fn format_efficiency(score: &EfficiencyScore) -> String {
    format!(
        "Session Efficiency: {}%\n\
         \n\
         Compression ratio:      {:.1}%\n\
         Tool choice efficiency: {:.1}%\n\
         Context reuse rate:     {:.1}%\n\
         Budget conservation:    {:.1}%",
        score.overall_bp / 100,
        score.compression_ratio_bp as f64 / 100.0,
        score.tool_choice_efficiency_bp as f64 / 100.0,
        score.context_reuse_rate_bp as f64 / 100.0,
        score.budget_utilization_bp as f64 / 100.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfect_compression() {
        let s = compute(1000, 0, 0, 1000, 0, 10, 150_000);
        assert_eq!(s.compression_ratio_bp, 10000);
    }

    #[test]
    fn no_compression() {
        let s = compute(1000, 1000, 0, 1000, 0, 10, 150_000);
        assert_eq!(s.compression_ratio_bp, 0);
    }

    #[test]
    fn half_agent_overhead() {
        let s = compute(1000, 500, 500, 1000, 0, 10, 150_000);
        assert_eq!(s.tool_choice_efficiency_bp, 5000);
    }

    #[test]
    fn full_dedup_reuse() {
        let s = compute(1000, 500, 0, 1000, 10, 10, 150_000);
        assert_eq!(s.context_reuse_rate_bp, 10000);
    }

    #[test]
    fn zero_calls() {
        let s = compute(0, 0, 0, 0, 0, 0, 150_000);
        assert_eq!(s.compression_ratio_bp, 0);
        assert_eq!(s.tool_choice_efficiency_bp, 10000);
        assert_eq!(s.context_reuse_rate_bp, 0);
    }

    #[test]
    fn overall_is_weighted_average() {
        let s = compute(1000, 500, 0, 50_000, 5, 10, 150_000);
        // compression: 50% = 5000bp
        // tool_choice: 100% = 10000bp (no agents)
        // reuse: 50% = 5000bp
        // budget: 1 - 50000/150000 = 66.7% = 6667bp
        let expected = (5000 * 30 + 10000 * 30 + 5000 * 20 + 6667 * 20) / 100;
        // Allow small rounding variance
        assert!((s.overall_bp as i64 - expected as i64).unsigned_abs() <= 5);
    }

    #[test]
    fn format_output() {
        let s = compute(1000, 300, 0, 50_000, 2, 10, 150_000);
        let out = format_efficiency(&s);
        assert!(out.contains("Session Efficiency:"));
        assert!(out.contains("Compression ratio:"));
    }
}
