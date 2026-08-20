//! Regression tests for issue #209 — a Windows Claude Code install where every
//! squeez hook was permanently inert while `squeez doctor` reported all-green.
//!
//! CI runs on macOS, so the Windows failure modes are covered by exercising the
//! pure string/path logic they come down to, with Windows-shaped inputs.

use squeez::hosts::settings_json::{
    has_buddy, has_squeez, hook_entry, is_buddy_cmd, normalize_sep, shell_arg, strip_buddy,
    upgrade_squeez_hook,
};
use squeez::json_util::{parse_value, JsonValue};
use std::path::PathBuf;

fn obj(json: &str) -> JsonValue {
    parse_value(json).unwrap()
}

/// Every command string registered under `event`.
fn commands(root: &JsonValue, event: &str) -> Vec<String> {
    root.get(event)
        .map(|v| v.as_arr())
        .unwrap_or(&[])
        .iter()
        .flat_map(|entry| entry.get("hooks").map(|h| h.as_arr()).unwrap_or(&[]))
        .map(|h| h.get_str("command").to_string())
        .collect()
}

// ── shell_arg ─────────────────────────────────────────────────────────────

#[test]
fn shell_arg_is_quoted_and_forward_slashed() {
    let p: PathBuf = ["C:\\Users\\JesseKlotz", ".claude", "squeez", "hooks"]
        .iter()
        .collect::<PathBuf>()
        .join("pretooluse.sh");
    let arg = shell_arg(&p);
    assert!(arg.starts_with('"') && arg.ends_with('"'), "must be quoted: {arg}");
    assert!(!arg.contains('\\'), "no backslash may survive: {arg}");
    assert!(arg.contains("C:/Users/JesseKlotz"), "drive path preserved: {arg}");
    assert!(arg.ends_with("pretooluse.sh\""), "script preserved: {arg}");
}

/// The reporter's exact symptom: bash treats each `\` as an escape, so the
/// unquoted form collapses to a path that does not exist. Quoting is what
/// stops it, and it must also survive a username containing a space.
#[test]
fn shell_arg_survives_a_username_with_a_space() {
    let arg = shell_arg(&PathBuf::from("/Users/Jesse Klotz/.claude/squeez/hooks/x.sh"));
    assert_eq!(arg, "\"/Users/Jesse Klotz/.claude/squeez/hooks/x.sh\"");
}

#[test]
fn normalize_sep_rewrites_every_separator() {
    assert_eq!(normalize_sep("a\\b\\c"), "a/b/c");
    assert_eq!(normalize_sep("a/b/c"), "a/b/c");
}

// ── separator-agnostic buddy detection ────────────────────────────────────

#[test]
fn buddy_is_detected_through_backslash_separators() {
    assert!(is_buddy_cmd("bash C:\\Users\\J\\.claude\\squeez\\buddy\\shims\\stop.sh"));
    assert!(is_buddy_cmd("bash \"C:/Users/J/.claude/squeez/buddy/shims/stop.sh\""));
    assert!(!is_buddy_cmd("bash \"C:/Users/J/.claude/squeez/hooks/pretooluse.sh\""));
}

/// The `setup` non-idempotency in the report: a Windows-written buddy shim
/// spells its path with `\`, the dedup check looked for `/buddy/`, so every
/// `squeez setup` appended another copy.
#[test]
fn windows_buddy_entry_is_not_re_added() {
    let entries = obj(
        r#"[{"hooks":[{"command":"bash C:\\Users\\J\\.claude\\squeez\\buddy\\shims\\stop.sh"}]}]"#,
    );
    assert!(has_buddy(entries.as_arr()), "backslash buddy entry must be seen");
    assert!(
        !has_squeez(entries.as_arr()),
        "a buddy entry must not satisfy the core-hook check, or core hooks stop being re-added"
    );
}

#[test]
fn windows_buddy_entry_is_stripped_on_uninstall() {
    let mut root = obj(
        r#"{"Stop":[{"hooks":[{"command":"bash C:\\Users\\J\\.claude\\squeez\\buddy\\shims\\stop.sh"}]}]}"#,
    );
    strip_buddy(&mut root, "Stop");
    assert!(root.get("Stop").is_none(), "buddy entry must be removed: {root:?}");
}

// ── upgrade in place ──────────────────────────────────────────────────────

/// The heart of the report: a broken registration survived every `squeez
/// update`, because "is a squeez hook present?" was true for the mangled
/// command too. It must be REWRITTEN, not merely tolerated or duplicated.
#[test]
fn broken_windows_registration_heals_instead_of_persisting() {
    let mut root = obj(
        r#"{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"bash C:\\Users\\J/.claude\\squeez\\hooks\\pretooluse.sh"}]}]}"#,
    );
    let fixed = "bash \"C:/Users/J/.claude/squeez/hooks/pretooluse.sh\"";
    upgrade_squeez_hook(&mut root, "PreToolUse", "pretooluse.sh", Some("Bash|Read"), fixed);

    let cmds = commands(&root, "PreToolUse");
    assert_eq!(cmds, vec![fixed.to_string()], "must rewrite, not append");
    assert_eq!(
        root.get("PreToolUse").unwrap().as_arr()[0].get_str("matcher"),
        "Bash|Read",
        "a stale matcher is upgraded too"
    );
}

#[test]
fn upgrade_appends_when_nothing_is_registered() {
    let mut root = obj("{}");
    let cmd = "bash \"/h/.claude/squeez/hooks/posttooluse.sh\"";
    upgrade_squeez_hook(&mut root, "PostToolUse", "posttooluse.sh", None, cmd);
    assert_eq!(commands(&root, "PostToolUse"), vec![cmd.to_string()]);
}

#[test]
fn upgrade_is_idempotent() {
    let mut root = obj("{}");
    let cmd = "bash \"/h/.claude/squeez/hooks/posttooluse.sh\"";
    for _ in 0..3 {
        upgrade_squeez_hook(&mut root, "PostToolUse", "posttooluse.sh", None, cmd);
    }
    assert_eq!(commands(&root, "PostToolUse").len(), 1);
}

#[test]
fn upgrade_leaves_foreign_hooks_alone() {
    let mut root = obj(
        r#"{"PostToolUse":[{"hooks":[{"type":"command","command":"bash /opt/other-tool/hook.sh"}]}]}"#,
    );
    let cmd = "bash \"/h/.claude/squeez/hooks/posttooluse.sh\"";
    upgrade_squeez_hook(&mut root, "PostToolUse", "posttooluse.sh", None, cmd);
    let cmds = commands(&root, "PostToolUse");
    assert_eq!(cmds.len(), 2, "foreign hook must survive: {cmds:?}");
    assert!(cmds.iter().any(|c| c.contains("other-tool")));
    assert!(cmds.iter().any(|c| c == cmd));
}

/// Two different squeez scripts under one event are distinct registrations —
/// upgrading one must not overwrite the other.
#[test]
fn upgrade_matches_on_the_script_not_just_on_squeez() {
    let mut root = obj("{}");
    let a = "bash \"/h/.claude/squeez/hooks/precompact.sh\"";
    let b = "bash \"/h/.claude/squeez/hooks/postcompact.sh\"";
    upgrade_squeez_hook(&mut root, "PreCompact", "precompact.sh", None, a);
    upgrade_squeez_hook(&mut root, "PreCompact", "postcompact.sh", None, b);
    assert_eq!(commands(&root, "PreCompact"), vec![a.to_string(), b.to_string()]);
}

#[test]
fn hook_entry_still_carries_matcher_first() {
    let e = hook_entry(Some("Bash"), "bash \"/h/x.sh\"");
    assert_eq!(e.get_str("matcher"), "Bash");
    assert_eq!(e.get("hooks").unwrap().as_arr()[0].get_str("command"), "bash \"/h/x.sh\"");
}
