use std::path::Path;

use crate::config::Config;
use crate::context::cache::SessionContext;
use crate::economy::agent_tracker;
use crate::session::{self, CurrentSession};

/// Entry point called from main.rs: `squeez track <tool> <bytes>`
pub fn run(tool: &str, bytes: &str) -> i32 {
    run_with_dir(tool, bytes, &session::sessions_dir())
}

/// `squeez track-spawn <tool>` — record a sub-agent spawn at DISPATCH time.
///
/// Called from the PreToolUse hook, before the agent runs. The old call site
/// was PostToolUse, which meant the counter only moved once an agent came back:
/// on 2026-08-21 sixteen agents were dispatched inside a 120s window against a
/// threshold of five, and the burst guard saw zero the entire time because none
/// of them had returned yet. A guard that can only fire after the spending is
/// finished is a report, not a guard.
pub fn run_spawn(tool: &str) -> i32 {
    run_spawn_with_dir(tool, &session::sessions_dir())
}

pub fn run_spawn_with_dir(tool: &str, sessions_dir: &Path) -> i32 {
    if !agent_tracker::is_agent_tool(tool) {
        return 0;
    }
    let cfg = Config::load();
    SessionContext::update(sessions_dir, |ctx| {
        let cost = agent_tracker::spawn_cost(ctx, &cfg);
        ctx.note_agent_spawn(tool, cost);
    });
    0
}

/// Testable version that accepts an explicit sessions directory.
pub fn run_with_dir(tool: &str, bytes: &str, sessions_dir: &Path) -> i32 {
    let tokens = bytes.parse::<u64>().unwrap_or(0) / 4;
    let mut current = match CurrentSession::load(sessions_dir) {
        Some(s) => s,
        None => return 0, // no session initialised — silent no-op
    };
    current.total_tokens += tokens;
    current.total_calls += 1;
    current.save(sessions_dir);

    let event = format!(
        "{{\"type\":\"tool\",\"tool\":\"{}\",\"tokens_est\":{},\"ts\":{}}}",
        crate::json_util::escape_str(tool),
        tokens,
        session::unix_now(),
    );
    session::append_event(sessions_dir, &current.session_file, &event);

    // ── Token economy: agent tracking + burn rate ─────────────────────
    SessionContext::update(sessions_dir, |ctx| {
        // Compaction drops earlier tool output from the model's context, but
        // context.json survives it. Raise the dedup floor so nothing recorded
        // before the compaction can be cited as "identical to #N" afterwards.
        if tool == "PreCompact" {
            ctx.dedup_floor_call = ctx.call_counter;
        }

        // Sub-agent spawns are NOT counted here. This runs at PostToolUse —
        // after the agent returns — so a parallel fan-out read as zero spawns
        // for exactly as long as it was in flight, which is the only window in
        // which a burst guard could act. Counting moved to `track-spawn`,
        // called from PreToolUse at dispatch. See agent_tracker.

        // Burn rate recording for non-Bash tools (Bash records via wrap.rs)
        if tokens > 0 {
            ctx.note_burn(tokens);
            ctx.note_tool_tokens(tool, tokens);
        }
    });
    0
}

/// `squeez track-agent-cost` — read a SubagentStop payload from stdin and
/// record what the finished sub-agent ACTUALLY cost, from its own transcript.
///
/// This is the point where estimation stops. `agent_spawn_cost` is a constant,
/// and even scaling it by observed context is a guess about a turn count the
/// hook cannot know. At SubagentStop the turns have already happened and the
/// agent's transcript is on disk, so the number can simply be read.
///
/// Silent no-op whenever the transcript is absent or carries no usage — a
/// missing measurement must leave the dispatch estimate standing, never zero it.
pub fn run_agent_cost() -> i32 {
    let mut buf = String::new();
    if std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf).is_err() {
        return 0;
    }
    run_agent_cost_with(&buf, &session::sessions_dir())
}

pub fn run_agent_cost_with(raw: &str, sessions_dir: &Path) -> i32 {
    let path = match agent_transcript_path(raw) {
        Some(p) => p,
        None => return 0,
    };
    let usage = match crate::context::transcript::measure_agent_usage(std::path::Path::new(&path)) {
        Some(u) => u,
        None => return 0,
    };
    SessionContext::update(sessions_dir, |ctx| {
        ctx.note_agent_measured(usage.total());
    });
    0
}

/// Locate the finished sub-agent's transcript.
///
/// Prefers the explicit `agent_transcript_path` the host provides. Falls back
/// to deriving it from the parent's `transcript_path` plus `agent_id`, since
/// Claude Code stores them at `<session>/subagents/agent-<id>.jsonl` — a
/// fallback worth having because the derived layout is observable on disk even
/// when the payload field is absent.
fn agent_transcript_path(raw: &str) -> Option<String> {
    if let Some(p) = crate::json_util::extract_str(raw, "agent_transcript_path") {
        if !p.is_empty() {
            return Some(p);
        }
    }
    let parent = crate::json_util::extract_str(raw, "transcript_path")?;
    let agent_id = crate::json_util::extract_str(raw, "agent_id")?;
    if agent_id.is_empty() {
        return None;
    }
    let stem = parent.strip_suffix(".jsonl")?;
    Some(format!("{}/subagents/agent-{}.jsonl", stem, agent_id))
}
