use std::path::Path;

use crate::config::Config;
use crate::context::cache::SessionContext;
use crate::economy::agent_tracker;
use crate::session::{self, CurrentSession};

/// Entry point called from main.rs: `squeez track <tool> <bytes>`
pub fn run(tool: &str, bytes: &str) -> i32 {
    run_with_dir(tool, bytes, &session::sessions_dir())
}

/// Testable version that accepts an explicit sessions directory.
pub fn run_with_dir(tool: &str, bytes: &str, sessions_dir: &Path) -> i32 {
    let tokens = bytes.parse::<u64>().unwrap_or(0) / 4;
    let mut current = match CurrentSession::load(sessions_dir) {
        Some(s) => s,
        None => return 0, // no session initialised — silent no-op
    };
    current.total_tokens += tokens;
    current.save(sessions_dir);

    let event = format!(
        "{{\"type\":\"tool\",\"tool\":\"{}\",\"tokens_est\":{},\"ts\":{}}}",
        crate::json_util::escape_str(tool),
        tokens,
        session::unix_now(),
    );
    session::append_event(sessions_dir, &current.session_file, &event);

    // ── Token economy: agent tracking + burn rate ─────────────────────
    let cfg = Config::load();
    let mut ctx = SessionContext::load(sessions_dir);

    // Sub-agent cost tracking
    if agent_tracker::is_agent_tool(tool) {
        ctx.note_agent_spawn(tool, cfg.agent_spawn_cost);
    }

    // Burn rate recording for non-Bash tools (Bash records via wrap.rs)
    if tokens > 0 {
        ctx.note_burn(tokens);
        ctx.note_tool_tokens(tool, tokens);
    }

    ctx.save(sessions_dir);
    0
}
