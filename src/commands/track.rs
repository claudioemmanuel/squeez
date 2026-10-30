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

/// `squeez track-filter <cmd> <emitted_chars>` — log a bash call whose output
/// a host plugin compressed with `squeez filter` (issue #292).
///
/// `squeez wrap` logs every call it runs; `squeez filter` is a pure stdin →
/// stdout pipe and logs nothing, so a host that compresses in an output hook
/// (Hermes) saved tokens that `squeez_session_summary` never saw. The plugin
/// calls this once it knows what it sent the model: the original output on
/// stdin, the length in chars of the text it emitted as `emitted_chars`. Pass
/// the original length to record output that was left as is.
pub fn run_filter(cmd: &str, emitted_chars: &str) -> i32 {
    let mut raw = String::new();
    if std::io::Read::read_to_string(&mut std::io::stdin(), &mut raw).is_err() {
        return 0;
    }
    let emitted = emitted_chars.parse::<usize>().unwrap_or(usize::MAX);
    run_filter_with(cmd, &raw, emitted, &session::sessions_dir(), &Config::load());
    0
}

/// Testable version of [`run_filter`]. Tokens use the same estimator as
/// `squeez wrap`; the emitted side is scaled from the input estimate by the
/// char ratio, since only the plugin holds the emitted text.
pub fn run_filter_with(
    cmd: &str,
    raw: &str,
    emitted_chars: usize,
    sessions_dir: &Path,
    config: &Config,
) {
    if CurrentSession::load(sessions_dir).is_none() {
        return; // no session initialised — silent no-op, like `track`
    }
    let in_tk = if config.class_density {
        crate::tokens::estimate_classed(raw, config.tokenizer_scale)
    } else {
        crate::tokens::estimate_scaled(raw, config.tokenizer_scale)
    };
    let raw_chars = raw.chars().count();
    let out_tk = if emitted_chars >= raw_chars || raw_chars == 0 {
        in_tk
    } else {
        (in_tk as u128 * emitted_chars as u128 / raw_chars as u128) as usize
    };

    let files = crate::commands::wrap::extract_file_paths(raw);
    let errors = crate::commands::wrap::extract_errors(raw);
    let git = crate::commands::wrap::extract_git_events_pub(cmd, raw);
    let test_sum = crate::commands::wrap::extract_test_summary(raw);

    let real_ctx_tokens = if config.context_cache_enabled {
        let mut measured = 0;
        SessionContext::update(sessions_dir, |ctx| {
            ctx.note_tool_tokens("Bash", in_tk as u64);
            ctx.note_burn(out_tk as u64);
            measured = ctx.real_ctx_tokens;
        });
        measured
    } else {
        SessionContext::load(sessions_dir).real_ctx_tokens
    };
    let _ = crate::commands::wrap::record_bash_event(
        sessions_dir, cmd, in_tk, out_tk, &files, &errors, &git, &test_sum, config,
        real_ctx_tokens, "",
    );
}
