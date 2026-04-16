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

/// Returns a warning string when cumulative agent token cost exceeds
/// `agent_warn_threshold_pct` of the context budget.
/// Budget = compact_threshold_tokens * 5 / 4 (same as intensity.rs).
pub fn agent_cost_warning(ctx: &SessionContext, cfg: &Config) -> Option<String> {
    if ctx.agent_spawns == 0 {
        return None;
    }
    let budget = cfg.compact_threshold_tokens * 5 / 4;
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
        ctx.agent_spawn_log.push(crate::context::cache::AgentSpawnEntry {
            call_n: 5,
            tool_name: "Agent".to_string(),
            estimated_tokens: 200_000,
            ts: 0,
        });
        ctx.agent_spawn_log.push(crate::context::cache::AgentSpawnEntry {
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
