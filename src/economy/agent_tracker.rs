use crate::config::Config;
use crate::context::cache::SessionContext;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Default estimated tokens consumed per sub-agent spawn (full context window).
pub const DEFAULT_AGENT_SPAWN_COST: u64 = 270_000;

/// Cap on tracked agent spawn entries (rolling window).
pub const MAX_AGENT_SPAWN_LOG: usize = 16;

// ── Detection ─────────────────────────────────────────────────────────────────

/// Returns true if `tool_name` is a sub-agent tool (Agent, Task).
pub fn is_agent_tool(tool_name: &str) -> bool {
    let lower = tool_name.to_lowercase();
    lower == "agent" || lower == "task"
}

// ── Warning ───────────────────────────────────────────────────────────────────

/// Estimated cost of one sub-agent spawn.
///
/// `agent_spawn_cost` is a flat compiled-in guess (350K). Measured against the
/// 2026-08-21 burn it was ~138x low: six directly-dispatched agents were costed
/// at 2.1M against a real ~290M, because each spawned its own descendants and
/// each descendant re-sent a growing context every turn.
///
/// A hook cannot know a sub-agent's future turn count, so this stays an
/// estimate — but it can stop ignoring what it does know. A sub-agent starts by
/// inheriting roughly the parent's live context and pays it again on every
/// turn, so the floor scales with the real window, not with a constant chosen
/// before 1M-token windows existed. Uses the larger of the configured constant
/// and the observed context, so this can only correct upward.
pub fn spawn_cost(ctx: &SessionContext, cfg: &Config) -> u64 {
    let observed = ctx.real_ctx_tokens.max(ctx.real_cache_read_tokens);
    if observed == 0 {
        return cfg.agent_spawn_cost;
    }
    // A sub-agent that does anything useful runs many turns, each re-sending
    // its whole context. Even a conservative multiplier beats a flat constant.
    observed.saturating_mul(4).max(cfg.agent_spawn_cost)
}

/// Returns a warning string when cumulative agent token cost exceeds
/// `agent_warn_threshold_pct` of the context budget.
pub fn agent_cost_warning(ctx: &SessionContext, cfg: &Config) -> Option<String> {
    if ctx.agent_spawns == 0 {
        return None;
    }
    // Must match intensity.rs, which honors a pinned `context_window_tokens`.
    // This used to hardcode `compact_threshold_tokens * 5 / 4` while claiming
    // in a comment to agree with intensity — so on a session with a pinned 1M
    // window the two disagreed by ~9x: intensity never escalated while this tag
    // fired from the very first spawn and could never escalate either.
    let budget = crate::context::intensity::budget_for(cfg, ctx.real_ctx_window);
    let threshold = (budget as f64 * cfg.agent_warn_threshold_pct as f64) as u64;
    if ctx.agent_estimated_tokens >= threshold {
        Some(format!(
            "[agents: {} calls, ~{}K est. tokens]",
            ctx.agent_spawns,
            ctx.agent_estimated_tokens / 1000,
        ))
    } else {
        None
    }
}

/// Returns a warning string when N or more agents were spawned within the
/// configured burst window. Fires once the count hits the threshold — callers
/// should print this as a separate line after the header.
pub fn burst_warning(ctx: &SessionContext, cfg: &Config) -> Option<String> {
    let threshold = cfg.parallel_agent_burst_threshold;
    if threshold == 0 || ctx.agent_spawn_log.is_empty() {
        return None;
    }
    let now = crate::session::unix_now();
    let window = cfg.parallel_agent_burst_window_secs;
    let burst_count = ctx
        .agent_spawn_log
        .iter()
        .filter(|e| now.saturating_sub(e.ts) <= window)
        .count();
    if burst_count >= threshold {
        let est_k = burst_count as u64 * cfg.agent_spawn_cost / 1000;
        Some(format!(
            "[squeez: WORKFLOW BURST — {} agents within {}s (~{}K tokens est.) \
             — reduce parallelism to stay within usage budget]",
            burst_count, window, est_k
        ))
    } else {
        None
    }
}

// ── MCP formatting ────────────────────────────────────────────────────────────

/// Format agent cost data for the MCP tool response.
pub fn format_agent_costs(ctx: &SessionContext) -> String {
    if ctx.agent_spawns == 0 {
        return "No sub-agent calls recorded this session.".to_string();
    }
    let mut out = format!(
        "Sub-agent usage: {} calls, ~{}K estimated tokens\n\n",
        ctx.agent_spawns,
        ctx.agent_estimated_tokens / 1000,
    );
    for entry in &ctx.agent_spawn_log {
        out.push_str(&format!(
            "  call#{} {} ~{}K tokens\n",
            entry.call_n,
            entry.tool_name,
            entry.estimated_tokens / 1000,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_agent_tool_positive() {
        assert!(is_agent_tool("Agent"));
        assert!(is_agent_tool("agent"));
        assert!(is_agent_tool("Task"));
        assert!(is_agent_tool("TASK"));
    }

    #[test]
    fn is_agent_tool_negative() {
        assert!(!is_agent_tool("Bash"));
        assert!(!is_agent_tool("Read"));
        assert!(!is_agent_tool("Grep"));
        assert!(!is_agent_tool("AgentSmith"));
    }

    #[test]
    fn warning_below_threshold_returns_none() {
        let ctx = SessionContext::default();
        let cfg = Config::default();
        assert!(agent_cost_warning(&ctx, &cfg).is_none());
    }

    #[test]
    fn warning_above_threshold() {
        let mut ctx = SessionContext::default();
        let cfg = Config::default();
        // Budget = 120_000 * 5/4 = 150_000. Threshold at 50% = 75_000.
        ctx.agent_spawns = 1;
        ctx.agent_estimated_tokens = 200_000;
        let warn = agent_cost_warning(&ctx, &cfg);
        assert!(warn.is_some());
        assert!(warn.unwrap().contains("200K"));
    }

    #[test]
    fn format_costs_empty() {
        let ctx = SessionContext::default();
        let out = format_agent_costs(&ctx);
        assert!(out.contains("No sub-agent"));
    }

    #[test]
    fn format_costs_with_entries() {
        let mut ctx = SessionContext::default();
        ctx.agent_spawns = 2;
        ctx.agent_estimated_tokens = 400_000;
        ctx.agent_spawn_log
            .push(crate::context::cache::AgentSpawnEntry {
                call_n: 5,
                tool_name: "Agent".to_string(),
                estimated_tokens: 200_000,
                ts: 0,
            });
        ctx.agent_spawn_log
            .push(crate::context::cache::AgentSpawnEntry {
                call_n: 10,
                tool_name: "Task".to_string(),
                estimated_tokens: 200_000,
                ts: 0,
            });
        let out = format_agent_costs(&ctx);
        assert!(out.contains("2 calls"));
        assert!(out.contains("400K"));
        assert!(out.contains("call#5"));
        assert!(out.contains("call#10"));
    }
}
