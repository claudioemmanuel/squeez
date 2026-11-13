//! Copilot CLI, Codex CLI and Gemini CLI tool hooks.
//!
//! Each host had its own Python with its own rules, and they are NOT the same
//! as Claude Code's — ported exactly, not routed through [`super::pre_tool_use`]:
//!
//! |              | Copilot             | Codex                  | Gemini                   |
//! |--------------|---------------------|------------------------|--------------------------|
//! | tool key     | `tool_name`         | `tool_name` or `tool`  | `tool_name` or `tool`    |
//! | input        | `tool_input`        | `tool_input`           | `tool_input` or `input`  |
//! | command      | `command`           | `command`              | `command` or `cmd`       |
//! | skip guard   | starts with bin     | + contains `squeez wrap` | same as Codex          |
//! | bypass       | `--no-squeez ` (space) | `--no-squeez`, lstrip | same as Codex          |
//! | output       | `hookSpecificOutput`| `hookSpecificOutput`   | top-level `decision`     |
//!
//! Copilot has no Agent/Task branch: the spawn ceiling and its ledger are
//! Claude Code's.

use super::{allow, isolated, pre_bash, pre_budget, response_len, shell_quote, HookEnv};
use crate::commands::{track, track_result};
use crate::config::Config;
use crate::json_util::{self, JsonValue};

#[derive(Clone, Copy)]
pub enum Host {
    Codex,
    Gemini,
}

const CODEX_SHELL_TOOLS: [&str; 8] = [
    "bash",
    "Bash",
    "shell",
    "Shell",
    "run_shell_command",
    "exec_command",
    "local_shell",
    "shell_command",
];
const GEMINI_SHELL_TOOLS: [&str; 3] = ["bash", "Bash", "run_shell_command"];

/// Python truthiness, for the `a or b or default` chains the scripts used.
fn truthy(v: &JsonValue) -> bool {
    match v {
        JsonValue::Null => false,
        JsonValue::Bool(b) => *b,
        JsonValue::Num(n) => *n != 0.0,
        JsonValue::Str(s) => !s.is_empty(),
        JsonValue::Arr(a) => !a.is_empty(),
        JsonValue::Obj(o) => !o.is_empty(),
    }
}

/// `d.get(k1) or d.get(k2) or …` — the first truthy value.
fn first_truthy<'a>(d: &'a JsonValue, keys: &[&str]) -> Option<&'a JsonValue> {
    keys.iter().filter_map(|k| d.get(k)).find(|v| truthy(v))
}

/// Copilot CLI PreToolUse: Claude Code's Bash and Read/Grep/Glob rules, and
/// nothing else.
pub fn copilot_pre(raw: &str, env: &HookEnv, cfg: &Config) -> Option<String> {
    if raw.trim().is_empty() {
        return None;
    }
    let mut d = json_util::parse_value(raw)?;
    let tool = d.get_str("tool_name").to_string();
    match tool.as_str() {
        "Bash" => pre_bash(&mut d, env, cfg),
        "Read" | "Grep" | "Glob" => pre_budget(&mut d, &tool, cfg),
        _ => None,
    }
}

/// Copilot CLI PostToolUse: track the size, then record the result. No
/// output rewrite.
pub fn copilot_post(raw: &str, env: &HookEnv, cfg: &Config) {
    let raw = raw.trim_end_matches('\n');
    let parsed = json_util::parse_value(raw);
    let tool = parsed
        .as_ref()
        .and_then(|d| d.get("tool_name"))
        .and_then(JsonValue::as_str)
        .unwrap_or("unknown")
        .to_string();
    // Claude Code's measure, not Copilot's older one: that counted any shape
    // it could not name (WebFetch, WebSearch, MCP) as 0 characters.
    let size = parsed.as_ref().map(response_len).unwrap_or(0);
    let dir = &env.sessions_dir;
    isolated(|| track::run_with_dir(&tool, &size.to_string(), dir));
    isolated(|| track_result::run_with_dir_cfg(&tool, raw, dir, cfg));
}

/// Codex CLI PreToolUse and Gemini CLI BeforeTool: rewrite a shell command to
/// `squeez wrap …`.
pub fn shell_pre(raw: &str, env: &HookEnv, cfg: &Config, host: Host) -> Option<String> {
    if raw.trim().is_empty() {
        return None;
    }
    let d = json_util::parse_value(raw)?;
    if !d.is_obj() {
        return None;
    }
    let tool = first_truthy(&d, &["tool_name", "tool"])
        .and_then(JsonValue::as_str)
        .unwrap_or("");
    let (tools, input_keys, cmd_keys): (&[&str], &[&str], &[&str]) = match host {
        Host::Codex => (&CODEX_SHELL_TOOLS, &["tool_input"], &["command"]),
        Host::Gemini => (
            &GEMINI_SHELL_TOOLS,
            &["tool_input", "input"],
            &["command", "cmd"],
        ),
    };
    if !tools.contains(&tool) {
        return None;
    }
    let mut input = first_truthy(&d, input_keys)?.clone();
    if !input.is_obj() {
        return None;
    }
    let cmd = first_truthy(&input, cmd_keys)?.as_str()?.to_string();
    if cmd.starts_with(&env.squeez_bin) || cmd.contains("squeez wrap") {
        return None;
    }
    let command = match cmd.strip_prefix("--no-squeez") {
        Some(rest) => rest.trim_start().to_string(),
        // Security gate (#150): leave risky/bypassed/wrap-disabled commands
        // untouched so the host's own permission flow sees the original.
        None if !cfg.should_wrap_bash(&cmd) => return None,
        None => format!("{} wrap {}", env.squeez_bin, shell_quote(&cmd)),
    };
    input.set("command", JsonValue::Str(command));
    Some(match host {
        // Codex rejects the older top-level shape ("hook returned invalid
        // pre-tool-use JSON output"); updatedInput for non-shell tools is still
        // unsupported (openai/codex#18491).
        Host::Codex => allow(&input),
        Host::Gemini => json_util::to_compact(&JsonValue::Obj(vec![
            ("decision".into(), JsonValue::Str("allow".into())),
            ("updatedInput".into(), input),
        ])),
    })
}

/// Codex CLI PostToolUse and Gemini CLI AfterTool: record the result into
/// SessionContext, print nothing. An empty or unparsable payload is ignored
/// outright (Claude Code's hook tracks it as `unknown`; these never did).
pub fn record_result(raw: &str, env: &HookEnv, cfg: &Config) {
    if raw.trim().is_empty() {
        return;
    }
    let Some(d) = json_util::parse_value(raw).filter(JsonValue::is_obj) else {
        return;
    };
    let tool = match first_truthy(&d, &["tool_name", "tool"]) {
        None => "unknown",
        Some(v) => match v.as_str() {
            Some(s) => s,
            // A non-string name made the Python subprocess call raise.
            None => return,
        },
    };
    isolated(|| track_result::run_with_dir_cfg(tool, raw, &env.sessions_dir, cfg));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn env() -> HookEnv {
        HookEnv {
            squeez_bin: "/bin/squeez".into(),
            ledger: PathBuf::from("/nonexistent/ledger"),
            sessions_dir: PathBuf::from("/nonexistent/sessions"),
            max_inflight: None,
            now: 0,
        }
    }

    fn pre(host: Host, payload: &str) -> Option<JsonValue> {
        shell_pre(payload, &env(), &Config::default(), host)
            .map(|s| json_util::parse_value(&s).unwrap())
    }

    fn cmd_of(v: &JsonValue, path: &[&str]) -> String {
        let mut cur = v;
        for k in path {
            cur = cur.get(k).unwrap();
        }
        cur.as_str().unwrap().to_string()
    }

    #[test]
    fn codex_wraps_every_shell_tool_name_and_nests_the_output() {
        for tool in CODEX_SHELL_TOOLS {
            let p = format!(r#"{{"tool":"{tool}","tool_input":{{"command":"ls -la"}}}}"#);
            let out = pre(Host::Codex, &p).expect(tool);
            assert_eq!(
                cmd_of(&out, &["hookSpecificOutput", "updatedInput", "command"]),
                "/bin/squeez wrap 'ls -la'"
            );
            assert_eq!(
                cmd_of(&out, &["hookSpecificOutput", "permissionDecision"]),
                "allow"
            );
        }
        assert!(pre(
            Host::Codex,
            r#"{"tool_name":"read_file","tool_input":{"command":"x"}}"#
        )
        .is_none());
    }

    #[test]
    fn gemini_reads_input_or_cmd_and_answers_top_level() {
        let out = pre(
            Host::Gemini,
            r#"{"tool_name":"run_shell_command","input":{"cmd":"git log"}}"#,
        )
        .unwrap();
        assert_eq!(cmd_of(&out, &["decision"]), "allow");
        // `cmd` is read, `command` is what gets written (appended).
        assert_eq!(
            cmd_of(&out, &["updatedInput", "command"]),
            "/bin/squeez wrap 'git log'"
        );
        assert_eq!(cmd_of(&out, &["updatedInput", "cmd"]), "git log");
        assert!(pre(
            Host::Gemini,
            r#"{"tool_name":"shell","tool_input":{"command":"ls"}}"#
        )
        .is_none());
    }

    #[test]
    fn bypass_strips_any_whitespace_and_guards_skip() {
        for host in [Host::Codex, Host::Gemini] {
            let out = pre(
                host,
                r#"{"tool_name":"bash","tool_input":{"command":"--no-squeez  echo hi"}}"#,
            )
            .unwrap();
            let s = json_util::to_compact(&out);
            assert!(s.contains(r#""command":"echo hi""#), "{s}");
            for skip in ["/bin/squeez wrap ls", "x; squeez wrap y", ""] {
                let p = format!(r#"{{"tool_name":"bash","tool_input":{{"command":"{skip}"}}}}"#);
                assert!(pre(host, &p).is_none(), "{skip}");
            }
            assert!(pre(host, r#"{"tool_name":"bash","tool_input":{"command":5}}"#).is_none());
            assert!(pre(host, r#"{"tool_name":"bash","tool_input":"ls"}"#).is_none());
            assert!(pre(host, "[1]").is_none());
        }
    }

    #[test]
    fn risky_commands_are_left_to_the_host() {
        assert!(pre(
            Host::Codex,
            r#"{"tool_name":"bash","tool_input":{"command":"rm -rf /"}}"#
        )
        .is_none());
    }

    #[test]
    fn copilot_has_no_agent_branch() {
        let out = copilot_pre(
            r#"{"tool_name":"Task","tool_input":{"prompt":"x"}}"#,
            &env(),
            &Config::default(),
        );
        assert!(out.is_none());
        let out = copilot_pre(
            r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#,
            &env(),
            &Config::default(),
        )
        .unwrap();
        assert!(out.contains(r#""command":"/bin/squeez wrap ls""#), "{out}");
    }
}
