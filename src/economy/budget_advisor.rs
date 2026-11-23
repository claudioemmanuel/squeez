//! Context budget advisor: speak up before an expensive command lands in a
//! nearly full context window.
//!
//! squeez already knows the two numbers that matter — how full the window is
//! (measured from the transcript) and what each program has cost per call in
//! the past (handler stats) — but only used them after the fact. When the
//! window is past `budget_advice_pct` used and the program about to run has
//! averaged at least `budget_advice_min_tokens` per call, the PreToolUse hook
//! attaches one advisory naming both numbers, once per program per session.
//! The command still runs; the advice shapes the agent's next choices.

use crate::config::Config;
use crate::context::cache::SessionContext;
use crate::economy::handler_stats::HandlerStats;

/// Advice text for running `handler` now, or `None`. Pure: all inputs given.
pub fn advise(
    handler: &str,
    used: u64,
    window: u64,
    avg_tokens: u64,
    calls: u64,
    cfg: &Config,
) -> Option<String> {
    if cfg.budget_advice_pct <= 0.0 || window == 0 || calls < 3 || handler.is_empty() {
        return None;
    }
    let used_pct = used as f64 * 100.0 / window as f64;
    if used_pct + 1e-6 < cfg.budget_advice_pct as f64 * 100.0 || avg_tokens < cfg.budget_advice_min_tokens {
        return None;
    }
    let left = window.saturating_sub(used);
    Some(format!(
        "[squeez budget: ~{:.0}% of the context window is used (~{}K tokens left); `{}` has averaged \
         ~{}K tokens per call in past sessions — narrow it (a path, a filter, -q, | tail) or write the \
         output to a file and read only what you need]",
        used_pct,
        left / 1000,
        handler,
        (avg_tokens / 1000).max(1)
    ))
}

/// The advisory for `handler` in the current session, recorded so it is
/// given once per handler per session.
pub fn advise_once(sessions_dir: &std::path::Path, handler: &str, cfg: &Config) -> Option<String> {
    let stats = HandlerStats::load(sessions_dir);
    let row = stats.rows().into_iter().find(|r| r.name == handler)?;
    let avg = row.out_tokens / row.calls.max(1);
    let mut out = None;
    SessionContext::update(sessions_dir, |ctx| {
        let window = crate::context::intensity::budget_for(cfg, ctx.real_ctx_window);
        let Some(advice) = advise(handler, ctx.real_ctx_tokens, window, avg, row.calls, cfg) else {
            return;
        };
        let key = format!("budget:{}:{handler}", ctx.session_epoch);
        if ctx.nudged_keys.contains(&key) {
            return;
        }
        ctx.nudged_keys.push(key);
        out = Some(advice);
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaks_only_when_full_and_expensive() {
        let cfg = Config::default();
        // 80% used, cargo averages 6K/call over 10 calls.
        let a = advise("cargo", 800_000, 1_000_000, 6_000, 10, &cfg).unwrap();
        assert!(a.contains("~80% of the context window") && a.contains("~200K tokens left"), "{a}");
        assert!(a.contains("`cargo` has averaged ~6K tokens"), "{a}");
        assert!(advise("cargo", 300_000, 1_000_000, 6_000, 10, &cfg).is_none(), "window not full");
        assert!(advise("ls", 800_000, 1_000_000, 200, 10, &cfg).is_none(), "cheap program");
        assert!(advise("cargo", 800_000, 1_000_000, 6_000, 2, &cfg).is_none(), "too few calls");
        let off = Config { budget_advice_pct: 0.0, ..Config::default() };
        assert!(advise("cargo", 800_000, 1_000_000, 6_000, 10, &off).is_none());
    }

    #[test]
    fn once_per_handler_per_session() {
        let dir = std::env::temp_dir().join(format!("squeez-budgetadv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = HandlerStats::default();
        for _ in 0..5 {
            s.record("cargo", 9_000, 5_000);
        }
        s.save(&dir);
        SessionContext::update(&dir, |c| {
            c.real_ctx_tokens = 900_000;
            c.real_ctx_window = 1_000_000;
        });
        let cfg = Config::default();
        assert!(advise_once(&dir, "cargo", &cfg).is_some());
        assert!(advise_once(&dir, "cargo", &cfg).is_none(), "said once");
        SessionContext::update(&dir, |c| c.begin_session("next"));
        assert!(advise_once(&dir, "cargo", &cfg).is_some(), "new session, said again");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
