//! Claude Code SubagentStop, the session-start settings check, and the
//! SessionStart context wrapper Codex CLI needs.

use std::path::{Path, PathBuf};

use super::{isolated, HookEnv};
use crate::commands::{compress_output, track, track_result};
use crate::config::Config;
use crate::json_util::{self, JsonValue};

/// SubagentStop: measure the sub-agent's real cost, release a spawn slot,
/// feed its final message into SessionContext, then log the event. Order is
/// the script's. compress-output prints the oversized-return advisory, which
/// is the only stdout.
pub fn subagent_stop(raw: &str, env: &HookEnv, cfg: &Config) {
    // The script captured stdin with `$(cat)`, which drops trailing newlines.
    let raw = raw.trim_end_matches('\n');
    let dir = &env.sessions_dir;
    // Where estimation stops: by now the turns have happened and are on disk.
    isolated(|| track::run_agent_cost_with(raw, dir));
    release_spawn_slot(&env.ledger);
    if let Some(wrapped) = as_tool_result(raw) {
        isolated(|| track_result::run_with_dir_cfg("SubagentStop", &wrapped, dir, cfg));
        // SubagentStop cannot rewrite the returned message (no updatedToolOutput
        // for Stop-family events), so this only nudges future sub-agents to
        // return a summary + file path.
        isolated(|| compress_output::run_with(&wrapped, "SubagentStop", dir, cfg));
    }
    // Spawns are counted at dispatch (`track-spawn`); the real cost was
    // recorded above from the transcript.
    isolated(|| track::run_with_dir("SubagentStop", "0", dir));
}

/// Drops the OLDEST stamp rather than matching an id: PreToolUse has no agent
/// id to record at dispatch time, and over-releasing is the safe direction — a
/// lost slot re-opens capacity, while a leaked one would throttle the session
/// until the TTL expires. No pruning here; the claim side prunes by TTL.
fn release_spawn_slot(ledger: &Path) {
    let Ok(body) = std::fs::read_to_string(ledger) else {
        return;
    };
    let mut stamps: Vec<&str> = body
        .split_whitespace()
        .filter(|x| x.bytes().all(|b| b.is_ascii_digit()))
        .collect();
    if stamps.is_empty() {
        return;
    }
    stamps.remove(0);
    let tmp = PathBuf::from(format!("{}.tmp", ledger.display()));
    if std::fs::write(&tmp, stamps.join("\n")).is_ok() {
        let _ = std::fs::rename(&tmp, ledger);
    }
}

/// `last_assistant_message` as a tool_result payload, so track-result's
/// content extraction picks it up. `None` when the payload is not an object.
fn as_tool_result(raw: &str) -> Option<String> {
    let d = json_util::parse_value(raw).filter(JsonValue::is_obj)?;
    let msg = match d.get("last_assistant_message") {
        None => String::new(),
        Some(JsonValue::Str(s)) => s.clone(),
        // Python's `str(None)`; any other non-string becomes its JSON text
        // (Python printed its repr — only the quoting differs).
        Some(JsonValue::Null) => "None".into(),
        Some(other) => json_util::to_compact(other),
    };
    let agent_id = d
        .get("agent_id")
        .cloned()
        .unwrap_or(JsonValue::Str(String::new()));
    Some(json_util::to_compact(&JsonValue::Obj(vec![
        ("tool_name".into(), JsonValue::Str("SubagentStop".into())),
        (
            "tool_result".into(),
            JsonValue::Obj(vec![("content".into(), JsonValue::Str(msg))]),
        ),
        ("agent_id".into(), agent_id),
    ])))
}

/// Codex CLI SessionStart: wrap `squeez init`'s banner in the documented
/// `additionalContext` shape. Nothing to say, nothing printed.
pub fn session_context(event: &str, context: &str) -> Option<String> {
    if context.is_empty() {
        return None;
    }
    Some(json_util::to_compact(&JsonValue::Obj(vec![(
        "hookSpecificOutput".into(),
        JsonValue::Obj(vec![
            ("hookEventName".into(), JsonValue::Str(event.into())),
            ("additionalContext".into(), JsonValue::Str(context.into())),
        ]),
    )])))
}

const HOOK_EVENTS: [&str; 5] = [
    "PreToolUse",
    "PostToolUse",
    "SessionStart",
    "UserPromptSubmit",
    "Stop",
];

/// The warning to print when `settings.json` exists but carries no squeez
/// hook — another tool (e.g. OMC) overwrote it on setup. Anything unreadable
/// or malformed stays silent: this is a nudge, not a diagnosis (`squeez
/// doctor` is that).
pub fn settings_check(path: &Path) -> Option<String> {
    let body = std::fs::read_to_string(path).ok()?;
    let d = json_util::parse_value(&body)?;
    if registered(&d) != Some(false) {
        return None;
    }
    Some(format!(
        "\n[squeez] WARNING: hooks not registered in {}\n\
         [squeez] Another tool may have overwritten your settings. Run: squeez setup\n\n",
        path.display()
    ))
}

/// `Some(true)` registered, `Some(false)` missing, `None` where the Python
/// raised (a shape it could not walk), which it reported as fine.
fn registered(d: &JsonValue) -> Option<bool> {
    if !d.is_obj() {
        return Some(true);
    }
    // Squeez writes hooks at top-level event keys; some other tools nest under
    // `hooks.*`. Check both shapes plus the statusLine slot.
    let nested = d
        .get("hooks")
        .filter(|h| h.is_obj())
        .map(JsonValue::obj_entries)
        .unwrap_or(&[]);
    let top = HOOK_EVENTS.iter().filter_map(|k| d.get(k));
    for entries in nested.iter().map(|(_, v)| v).chain(top) {
        for e in entries.as_arr() {
            let hooks = match e.get("hooks") {
                _ if !e.is_obj() => continue,
                None => continue,
                Some(JsonValue::Arr(items)) => items.as_slice(),
                // Iterating an empty dict or string is a no-op; anything else
                // made the Python raise.
                Some(JsonValue::Obj(f)) if f.is_empty() => continue,
                Some(JsonValue::Str(s)) if s.is_empty() => continue,
                Some(_) => return None,
            };
            for h in hooks {
                if !h.is_obj() {
                    return None;
                }
                if mentions_squeez(h.get("command")) {
                    return Some(true);
                }
            }
        }
    }
    let sl = d.get("statusLine").filter(|s| s.is_obj());
    Some(sl.is_some_and(|s| mentions_squeez(s.get("command"))))
}

/// `'squeez' in str(h.get('command', ''))`.
fn mentions_squeez(cmd: Option<&JsonValue>) -> bool {
    match cmd {
        None => false,
        Some(JsonValue::Str(s)) => s.contains("squeez"),
        Some(other) => json_util::to_compact(other).contains("squeez"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("squeez-lifecycle-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn check(body: &str) -> bool {
        let d = tmp(&format!(
            "{:x}",
            crate::context::hash::fnv1a_64(body.as_bytes())
        ));
        let p = d.join("settings.json");
        std::fs::write(&p, body).unwrap();
        let out = settings_check(&p).is_some();
        let _ = std::fs::remove_dir_all(&d);
        out
    }

    #[test]
    fn settings_check_warns_only_when_squeez_is_absent_from_a_walkable_object() {
        // missing
        assert!(check("{}"));
        assert!(check(r#"{"PreToolUse":[{"hooks":[{"command":"other"}]}]}"#));
        assert!(check(
            r#"{"hooks":{"PreToolUse":[{"hooks":[]}]},"statusLine":{"command":"hud"}}"#
        ));
        // registered, in each place squeez or another tool may put it
        assert!(!check(
            r#"{"PreToolUse":[{"hooks":[{"command":"bash ~/.claude/squeez/hooks/pretooluse.sh"}]}]}"#
        ));
        assert!(!check(
            r#"{"hooks":{"PreToolUse":[{"hooks":[{"command":"squeez"}]}]}}"#
        ));
        assert!(!check(
            r#"{"statusLine":{"command":"bash ~/.claude/squeez/hooks/statusline.sh"}}"#
        ));
        // unwalkable or not an object: the Python reported "ok"
        assert!(!check("[]"));
        assert!(!check("not json"));
        assert!(!check(r#"{"PreToolUse":[{"hooks":["str"]}]}"#));
        assert!(!check(r#"{"PreToolUse":[{"hooks":null}]}"#));
        // squeez found before the unwalkable entry wins
        assert!(!check(
            r#"{"PreToolUse":[{"hooks":[{"command":"squeez"}, "x"]}]}"#
        ));
        assert!(settings_check(Path::new("/nonexistent/settings.json")).is_none());
    }

    #[test]
    fn release_drops_the_oldest_stamp_only() {
        let d = tmp("ledger");
        let p = d.join("inflight_agents");
        std::fs::write(&p, "100\n200\njunk\n300").unwrap();
        release_spawn_slot(&p);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "200\n300");
        std::fs::write(&p, "").unwrap();
        release_spawn_slot(&p);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn subagent_message_becomes_a_tool_result() {
        let v = json_util::parse_value(
            &as_tool_result(r#"{"last_assistant_message":"done","agent_id":"a1"}"#).unwrap(),
        )
        .unwrap();
        assert_eq!(v.get_str("tool_name"), "SubagentStop");
        assert_eq!(v.get("tool_result").unwrap().get_str("content"), "done");
        assert_eq!(v.get_str("agent_id"), "a1");
        let v = json_util::parse_value(&as_tool_result("{}").unwrap()).unwrap();
        assert_eq!(v.get("tool_result").unwrap().get_str("content"), "");
        assert!(as_tool_result("[]").is_none());
        assert!(as_tool_result("nope").is_none());
    }

    #[test]
    fn session_context_wraps_or_stays_silent() {
        assert!(session_context("SessionStart", "").is_none());
        let v =
            json_util::parse_value(&session_context("SessionStart", "hi\n\"x\"").unwrap()).unwrap();
        let h = v.get("hookSpecificOutput").unwrap();
        assert_eq!(h.get_str("hookEventName"), "SessionStart");
        assert_eq!(h.get_str("additionalContext"), "hi\n\"x\"");
    }
}
