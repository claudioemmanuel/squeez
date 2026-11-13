//! `squeez hook <event>` — every host hook's logic, run in one process.
//!
//! This used to be Python embedded in the `hooks/*.sh` scripts: an interpreter
//! probe, one or two Python runs, and up to three more squeez processes per
//! event — on Claude Code ~40 ms before a tool started and ~100 ms after it
//! finished, measured. The scripts now only find the binary and call this, so
//! no hook needs an interpreter. Behaviour is a line-for-line port; the Python
//! is in git history (`git show v1.49.1:hooks/pretooluse.sh`).
//!
//! - this file: Claude Code PreToolUse / PostToolUse
//! - [`shell`]: Copilot CLI, Codex CLI and Gemini CLI tool hooks
//! - [`lifecycle`]: SubagentStop, the session-start settings check, and the
//!   SessionStart context wrapper
//! - [`statusline`]: the Claude Code status line
//!
//! Fails open: any parse error or missing field means "print nothing", which
//! leaves the tool call untouched. The only thing that blocks a call is the
//! JSON `deny` of the agent-spawn ceiling. The exit code is always 0.

use std::io::Read;
use std::path::PathBuf;

use crate::commands::{compress_output, compress_prompt, track, track_result};
use crate::config::Config;
use crate::economy::budget;
use crate::json_util::{self, JsonValue};
use crate::session;

pub mod lifecycle;
pub mod shell;
pub mod statusline;

/// A spawn older than this is presumed finished (its SubagentStop was lost).
const SPAWN_TTL_SECS: i64 = 3600;

/// What the hook reads from its environment, made explicit for tests.
pub struct HookEnv {
    /// The binary path the script resolved. The rewritten Bash command and the
    /// double-wrap guard both use this exact string.
    pub squeez_bin: String,
    /// In-flight spawn ledger: claimed here, released by [`lifecycle::subagent_stop`].
    pub ledger: PathBuf,
    pub sessions_dir: PathBuf,
    /// `SQUEEZ_MAX_INFLIGHT_AGENTS`, unparsed.
    pub max_inflight: Option<String>,
    pub now: i64,
}

impl HookEnv {
    pub fn from_process() -> Self {
        let squeez_bin = std::env::var("SQUEEZ_BIN").unwrap_or_else(|_| {
            std::env::current_exe()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        });
        // `~/.claude/squeez/sessions/inflight_agents`, deliberately not under
        // SQUEEZ_DIR: SubagentStop releases slots from this exact path.
        let ledger =
            PathBuf::from(session::home_dir()).join(".claude/squeez/sessions/inflight_agents");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        HookEnv {
            squeez_bin,
            ledger,
            sessions_dir: session::sessions_dir(),
            max_inflight: std::env::var("SQUEEZ_MAX_INFLIGHT_AGENTS").ok(),
            now,
        }
    }
}

pub fn run(args: &[String]) -> i32 {
    let event = args.first().map(String::as_str).unwrap_or("");
    // These two read files, not stdin: a status line run by hand would block.
    match event {
        "statusline" => {
            if let Some(line) = statusline::render(&statusline_dir()) {
                println!("{line}");
            }
            return 0;
        }
        "settings-check" => {
            let path = PathBuf::from(session::home_dir()).join(".claude/settings.json");
            if let Some(warning) = lifecycle::settings_check(&path) {
                print!("{warning}");
            }
            return 0;
        }
        "pre-tool-use"
        | "post-tool-use"
        | "copilot-pre-tool-use"
        | "copilot-post-tool-use"
        | "codex-pre-tool-use"
        | "gemini-before-tool"
        | "record-result"
        | "subagent-stop"
        | "session-context" => {}
        // Before reading stdin, which would block a bare `squeez hook`.
        _ => {
            eprintln!(
                "usage: squeez hook <pre-tool-use|post-tool-use|copilot-pre-tool-use|\
                 copilot-post-tool-use|codex-pre-tool-use|gemini-before-tool|record-result|\
                 subagent-stop|session-context <Event>|settings-check|statusline>"
            );
            return 0;
        }
    }
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    let env = HookEnv::from_process();
    let cfg = Config::load();
    let out = match event {
        "pre-tool-use" => pre_tool_use(&raw, &env, &cfg),
        "post-tool-use" => {
            post_tool_use(&raw, &env, &cfg);
            None
        }
        "copilot-pre-tool-use" => shell::copilot_pre(&raw, &env, &cfg),
        "copilot-post-tool-use" => {
            shell::copilot_post(&raw, &env, &cfg);
            None
        }
        "codex-pre-tool-use" => shell::shell_pre(&raw, &env, &cfg, shell::Host::Codex),
        "gemini-before-tool" => shell::shell_pre(&raw, &env, &cfg, shell::Host::Gemini),
        "record-result" => {
            shell::record_result(&raw, &env, &cfg);
            None
        }
        "subagent-stop" => {
            lifecycle::subagent_stop(&raw, &env, &cfg);
            None
        }
        "session-context" => lifecycle::session_context(
            args.get(1).map(String::as_str).unwrap_or("SessionStart"),
            &raw,
        ),
        _ => None,
    };
    if let Some(out) = out {
        println!("{out}");
    }
    0
}

/// `$SQUEEZ_DIR`, or `~/.claude/squeez` when unset OR EMPTY: the status-line
/// script used `${SQUEEZ_DIR:-…}`, which treats empty as unset.
fn statusline_dir() -> PathBuf {
    match std::env::var("SQUEEZ_DIR") {
        Ok(d) if !d.is_empty() => PathBuf::from(d),
        _ => PathBuf::from(session::home_dir()).join(".claude/squeez"),
    }
}

// ── PreToolUse ───────────────────────────────────────────────────────────────

/// The hook's stdout, or `None` to leave the call untouched.
pub fn pre_tool_use(raw: &str, env: &HookEnv, cfg: &Config) -> Option<String> {
    if raw.trim().is_empty() {
        return None;
    }
    let mut d = json_util::parse_value(raw)?;
    let tool = d
        .get("tool_name")
        .and_then(JsonValue::as_str)
        .unwrap_or("")
        .to_string();
    match tool.as_str() {
        "Bash" => pre_bash(&mut d, env, cfg),
        "Read" | "Grep" | "Glob" => pre_budget(&mut d, &tool, cfg),
        "Agent" | "Task" => pre_agent(&mut d, &tool, env, cfg),
        _ => None,
    }
}

pub(super) fn pre_bash(d: &mut JsonValue, env: &HookEnv, cfg: &Config) -> Option<String> {
    let input = d.get_mut("tool_input").filter(|v| v.is_obj())?;
    let cmd = input.get("command")?.as_str()?.to_string();
    if cmd.starts_with(&env.squeez_bin) {
        return None;
    }
    if let Some(rest) = cmd.strip_prefix("--no-squeez ") {
        input.set("command", JsonValue::Str(rest.to_string()));
        return Some(allow(input));
    }
    // Security gate (#150): only rewrite to `squeez wrap …` when squeez deems
    // it safe. Risky (rm -rf, git push --force, …), bypassed, or wrap-disabled
    // commands are left UNTOUCHED — no updatedInput, no permissionDecision — so
    // the host evaluates the user's native deny/ask rules against the original.
    if !cfg.should_wrap_bash(&cmd) {
        return None;
    }
    let wrapped = format!("{} wrap {}", env.squeez_bin, shell_quote(&cmd));
    input.set("command", JsonValue::Str(wrapped));
    Some(allow(input))
}

/// Read/Grep/Glob: inject the configured limits, never overriding a value the
/// caller set explicitly. Best-effort.
pub(super) fn pre_budget(d: &mut JsonValue, tool: &str, cfg: &Config) -> Option<String> {
    let patch = json_util::parse_value(&budget::budget_params(tool, cfg)?)?;
    if d.get("tool_input").is_none() {
        d.set("tool_input", JsonValue::Obj(Vec::new()));
    }
    let input = d.get_mut("tool_input").filter(|v| v.is_obj())?;
    let mut changed = false;
    for (k, v) in patch.obj_entries() {
        if input.get(k).is_none() {
            input.set(k, v.clone());
            changed = true;
        }
    }
    changed.then(|| allow(input))
}

/// Agent/Task: enforce the concurrent-spawn ceiling, record the spawn at
/// dispatch, then compress the prompt.
///
/// Why a DENY and not a warning: on 2026-08-21 a single unattended prompt
/// fanned out to 26 agents and burned ~302M tokens in 41 minutes. squeez
/// detected the burst and said nothing the model was obliged to obey. A
/// ceiling that is not enforced is documentation.
fn pre_agent(d: &mut JsonValue, tool: &str, env: &HookEnv, cfg: &Config) -> Option<String> {
    // An unparsable override aborted the Python before it did anything.
    let max_inflight: i64 = match &env.max_inflight {
        Some(v) => v.trim().parse().ok()?,
        None => 5,
    };
    if max_inflight > 0 {
        if let Some(deny) = claim_spawn_slot(env, max_inflight) {
            return Some(deny);
        }
    }
    // Counting at PostToolUse (on return) left the burst guard reading zero
    // for exactly as long as a fan-out was in flight.
    track::run_spawn_with_dir(tool, &env.sessions_dir);

    let input = d.get_mut("tool_input").filter(|v| v.is_obj())?;
    let prompt = input.get("prompt")?.as_str()?.to_string();
    if prompt.is_empty() {
        return None;
    }
    let compressed = compress_prompt::compress(&prompt, cfg);
    if compressed.is_empty() || compressed.chars().count() >= prompt.chars().count() {
        return None;
    }
    input.set("prompt", JsonValue::Str(compressed));
    Some(allow(input))
}

/// The ledger is one unix timestamp per in-flight spawn, pruned by TTL on
/// every read, so a sub-agent that dies without SubagentStop can never wedge
/// the gate shut. Returns the deny JSON when full, else records this spawn.
/// Any I/O error fails open.
fn claim_spawn_slot(env: &HookEnv, max_inflight: i64) -> Option<String> {
    if let Some(dir) = env.ledger.parent() {
        std::fs::create_dir_all(dir).ok()?;
    }
    let mut stamps: Vec<i64> = std::fs::read_to_string(&env.ledger)
        .unwrap_or_default()
        .split_whitespace()
        .filter(|x| x.bytes().all(|b| b.is_ascii_digit()))
        .filter_map(|x| x.parse().ok())
        .filter(|s| env.now - s < SPAWN_TTL_SECS)
        .collect();
    if stamps.len() as i64 >= max_inflight {
        let reason = format!(
            "squeez: {} sub-agents already in flight (ceiling {}). Refusing this spawn.\n\
             Each sub-agent pays its own full context floor (~66K tokens) and re-sends its \
             whole context every turn, so a wide fan-out costs multiples of the visible work. \
             Wait for a running agent to finish, or do this step inline.\n\
             Raise deliberately with SQUEEZ_MAX_INFLIGHT_AGENTS=<n>; 0 disables the ceiling.",
            stamps.len(),
            max_inflight
        );
        let out = JsonValue::Obj(vec![(
            "hookSpecificOutput".into(),
            JsonValue::Obj(vec![
                ("hookEventName".into(), JsonValue::Str("PreToolUse".into())),
                ("permissionDecision".into(), JsonValue::Str("deny".into())),
                ("permissionDecisionReason".into(), JsonValue::Str(reason)),
            ]),
        )]);
        return Some(json_util::to_compact(&out));
    }
    stamps.push(env.now);
    let body: Vec<String> = stamps.iter().map(i64::to_string).collect();
    let tmp = PathBuf::from(format!(
        "{}.{}.tmp",
        env.ledger.display(),
        std::process::id()
    ));
    if std::fs::write(&tmp, body.join("\n")).is_ok() {
        let _ = std::fs::rename(&tmp, &env.ledger);
    }
    None
}

pub(super) fn allow(updated_input: &JsonValue) -> String {
    let out = JsonValue::Obj(vec![(
        "hookSpecificOutput".into(),
        JsonValue::Obj(vec![
            ("hookEventName".into(), JsonValue::Str("PreToolUse".into())),
            ("permissionDecision".into(), JsonValue::Str("allow".into())),
            ("updatedInput".into(), updated_input.clone()),
        ]),
    )]);
    json_util::to_compact(&out)
}

/// Python's `shlex.quote`, exactly. The wrapped command is auto-allowed, so a
/// quoting difference here would be a command-injection bug.
pub fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    if s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&b))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

// ── PostToolUse ──────────────────────────────────────────────────────────────

/// Tools whose output `compress-output` may rewrite via updatedToolOutput.
fn rewritable(tool: &str) -> bool {
    tool.starts_with("mcp__")
        || matches!(
            tool,
            "Read"
                | "Grep"
                | "Glob"
                | "Monitor"
                | "Agent"
                | "Task"
                | "Edit"
                | "Write"
                | "Skill"
                | "WebFetch"
                | "WebSearch"
                | "ToolSearch"
                | "TaskOutput"
        )
}

/// track → compress-output → track-result, in that order. compress-output must
/// run BEFORE track-result: its stale-state guard reads the file's access flag
/// from the PREVIOUS call, which track-result for this call would overwrite.
/// Each stage is isolated, as `|| true` isolated the separate processes.
pub fn post_tool_use(raw: &str, env: &HookEnv, cfg: &Config) {
    // The script captured stdin with `$(cat)`, which drops trailing newlines.
    let raw = raw.trim_end_matches('\n');
    let parsed = json_util::parse_value(raw);
    let tool = parsed
        .as_ref()
        .and_then(|d| d.get("tool_name"))
        .and_then(JsonValue::as_str)
        .unwrap_or("unknown")
        .to_string();
    let size = parsed.as_ref().map(response_len).unwrap_or(0);
    let dir = &env.sessions_dir;

    isolated(|| track::run_with_dir(&tool, &size.to_string(), dir));
    if rewritable(&tool) {
        isolated(|| compress_output::run_with(raw, &tool, dir, cfg));
    }
    isolated(|| track_result::run_with_dir_cfg(&tool, raw, dir, cfg));
}

pub(super) fn isolated(stage: impl FnOnce() -> i32) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(stage));
}

/// Characters of text in the tool's response (`tool_response`, falling back
/// to the legacy `tool_result`).
pub fn response_len(d: &JsonValue) -> u64 {
    match d.get("tool_response").or_else(|| d.get("tool_result")) {
        Some(r) => text_len(r),
        None => 0,
    }
}

/// Shapes: plain string, `{content: str}`, `{content: [{type:text,text:…}]}`,
/// and the Read shape `{type:text, file:{content: …}}`.
fn text_len(c: &JsonValue) -> u64 {
    match c {
        JsonValue::Null => 0,
        JsonValue::Str(s) => s.chars().count() as u64,
        // Blocks are usually {type:text,text:…} but a list of plain strings
        // or nested shapes is just as valid — recurse instead of assuming.
        JsonValue::Arr(items) => items.iter().map(text_len).sum(),
        JsonValue::Obj(fields) => {
            if let Some(content) = c.get("content") {
                return text_len(content);
            }
            if let Some(file) = c.get("file").filter(|f| f.is_obj()) {
                return file.get("content").map(text_len).unwrap_or(0);
            }
            if let Some(text) = c.get("text") {
                return text_len(text);
            }
            // Binary blocks carry no text. Image dedup runs off `image_fp` in
            // compress-output; counting base64 as prose would turn every
            // screenshot into a fake 100K result.
            let binary = matches!(c.get_str("type"), "image" | "video" | "audio" | "document");
            if binary || c.get("source").is_some() {
                return 0;
            }
            // UNKNOWN SHAPE. Returning 0 here made squeez blind to the tools
            // that cost the most: WebFetch/WebSearch answer with neither
            // `content` nor `text`, so all 797 calls of the 2026-08-21 burn
            // were recorded as 0 tokens. Measure what we cannot name.
            fields.iter().map(|(_, v)| text_len(v)).sum()
        }
        // Python's `len(str(x))`: True/False, and ints without a decimal point.
        JsonValue::Bool(b) => {
            if *b {
                4
            } else {
                5
            }
        }
        JsonValue::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => {
            (*n as i64).to_string().len() as u64
        }
        JsonValue::Num(n) => n.to_string().len() as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn len_of(payload: &str) -> u64 {
        response_len(&json_util::parse_value(payload).unwrap())
    }

    /// Real Claude Code payload shapes (the tokens_est:0 outage was a hook
    /// reading `tool_result` while Claude Code sends `tool_response`).
    #[test]
    fn response_len_covers_real_payload_shapes() {
        assert_eq!(
            len_of(r#"{"tool_name":"Bash","tool_response":"plain string output 1234"}"#),
            24
        );
        assert_eq!(
            len_of(
                r#"{"tool_name":"Read","tool_response":{"type":"text","file":{"filePath":"/tmp/a.rs","content":"hello world content here","numLines":1}}}"#
            ),
            24
        );
        assert_eq!(
            len_of(
                r#"{"tool_response":{"content":[{"type":"text","text":"block one"},{"type":"text","text":"block two"}]}}"#
            ),
            18
        );
        assert_eq!(
            len_of(
                r#"{"tool_response":{"content":[{"type":"image","source":{"type":"base64","data":"/9j/4AAQ"}}]}}"#
            ),
            0
        );
        assert_eq!(
            len_of(r#"{"tool_name":"Read","tool_result":{"content":"legacy shape"}}"#),
            12
        );
        // Unknown shape: every value measured; chars, not bytes; True → 4, 200 → 3.
        assert_eq!(
            len_of(r#"{"tool_response":{"result":"abc é","code":200,"ok":true}}"#),
            5 + 3 + 4
        );
        assert_eq!(len_of(r#"{"tool_name":"X"}"#), 0);
    }

    #[test]
    fn shell_quote_matches_python_shlex() {
        assert_eq!(shell_quote(""), "''");
        assert_eq!(
            shell_quote("git_status-v1.2/a@b%c+d=e:f,g"),
            "git_status-v1.2/a@b%c+d=e:f,g"
        );
        assert_eq!(shell_quote("ls -la"), "'ls -la'");
        assert_eq!(shell_quote("it's"), "'it'\"'\"'s'");
        assert_eq!(shell_quote("olá"), "'olá'");
        assert_eq!(shell_quote("$(rm -rf /)"), "'$(rm -rf /)'");
    }

    fn env_in(dir: &std::path::Path, max: Option<&str>) -> HookEnv {
        HookEnv {
            squeez_bin: "/bin/squeez".into(),
            ledger: dir.join("inflight_agents"),
            sessions_dir: dir.to_path_buf(),
            max_inflight: max.map(str::to_string),
            now: 1_000_000,
        }
    }

    fn tmp() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!(
            "squeez_hook_unit_{}_{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn bash_guards_and_no_squeez() {
        let dir = tmp();
        let env = env_in(&dir, None);
        let cfg = Config::default();
        let pre = |p: &str| pre_tool_use(p, &env, &cfg);
        assert_eq!(pre(""), None);
        assert_eq!(pre("{bad"), None);
        assert_eq!(
            pre(r#"{"tool_name":"Bash","tool_input":{"command":"/bin/squeez wrap ls"}}"#),
            None
        );
        assert_eq!(
            pre(r#"{"tool_name":"Bash","tool_input":{"command":7}}"#),
            None
        );
        let out =
            pre(r#"{"tool_name":"Bash","tool_input":{"command":"--no-squeez git log"}}"#).unwrap();
        assert_eq!(
            out,
            r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{"command":"git log"}}}"#
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn budget_never_overrides_an_explicit_value() {
        let dir = tmp();
        let env = env_in(&dir, None);
        let cfg = Config {
            read_max_lines: 300,
            ..Config::default()
        };
        let out = pre_tool_use(
            r#"{"tool_name":"Read","tool_input":{"file_path":"/x"}}"#,
            &env,
            &cfg,
        )
        .unwrap();
        assert!(
            out.contains(r#""updatedInput":{"file_path":"/x","limit":300}"#),
            "{out}"
        );
        assert_eq!(
            pre_tool_use(
                r#"{"tool_name":"Read","tool_input":{"file_path":"/x","limit":5}}"#,
                &env,
                &cfg
            ),
            None
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spawn_ceiling_denies_when_full_and_prunes_stale_stamps() {
        let dir = tmp();
        let env = env_in(&dir, Some("2"));
        let cfg = Config::default();
        let agent = r#"{"tool_name":"Agent","tool_input":{"prompt":"x"}}"#;
        // One stale stamp (older than the TTL) does not count.
        std::fs::write(&env.ledger, "1\n999990").unwrap();
        assert_eq!(pre_tool_use(agent, &env, &cfg), None);
        assert_eq!(
            std::fs::read_to_string(&env.ledger).unwrap(),
            "999990\n1000000"
        );
        let deny = pre_tool_use(agent, &env, &cfg).unwrap();
        assert!(deny.contains(r#""permissionDecision":"deny""#), "{deny}");
        assert!(
            deny.contains("2 sub-agents already in flight (ceiling 2)"),
            "{deny}"
        );
        // An unparsable ceiling does nothing at all, like the Python it replaced.
        let bad = env_in(&dir, Some("abc"));
        assert_eq!(pre_tool_use(agent, &bad, &cfg), None);
        assert_eq!(
            std::fs::read_to_string(&env.ledger).unwrap(),
            "999990\n1000000"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
