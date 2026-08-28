//! Shared JSON settings patching for the host adapters.
//!
//! Every adapter registers itself by read-modify-writing a host-owned JSON
//! file (`~/.claude/settings.json`, `~/.gemini/settings.json`, …). That merge
//! used to shell out to `python3 -c <script>`, which made `squeez setup` fail
//! outright on any machine without a `python3` on PATH — notably Windows,
//! where Python installs as `python.exe`/`py.exe` and the spawn fails with
//! Rust's `program not found` (see issue #190).
//!
//! The merge is now pure Rust on top of `json_util`, so setup has no runtime
//! interpreter dependency at all.

use crate::json_util::{self, JsonValue};
use std::path::Path;

/// Outcome of reading a settings file that squeez is about to rewrite.
pub enum Existing {
    /// File is absent — start from `{}`.
    Missing,
    /// File parsed into a JSON object.
    Object(JsonValue),
}

/// Read a settings file, refusing to touch anything squeez cannot safely
/// round-trip. A malformed or non-object file is an error rather than a
/// silent overwrite — clobbering a user's editor settings is unrecoverable.
pub fn load(path: &Path) -> Result<Existing, String> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Existing::Missing),
        Err(e) => return Err(format!("could not read {}: {e}", path.display())),
    };
    // Python read these with encoding="utf-8-sig"; strip the BOM to match.
    let trimmed = raw.trim_start_matches('\u{feff}').trim();
    if trimmed.is_empty() {
        return Ok(Existing::Missing);
    }
    let Some(value) = json_util::parse_value(trimmed) else {
        return Err(format!(
            "refusing to overwrite {}: could not parse existing JSON.\n\
             squeez: fix or remove the file, then re-run `squeez setup`",
            path.display()
        ));
    };
    if !value.is_obj() {
        return Err(format!(
            "refusing to overwrite {}: top-level value is not a JSON object",
            path.display()
        ));
    }
    Ok(Existing::Object(value))
}

/// Like [`load`], but for the uninstall path: an unreadable or malformed file
/// is nothing to clean up, so it yields `None` instead of an error.
pub fn load_lenient(path: &Path) -> Option<JsonValue> {
    match load(path) {
        Ok(Existing::Object(v)) => Some(v),
        _ => None,
    }
}

/// Write `value` back, preserving the previous contents as `<path>.bak` and
/// swapping through a temp file so a crash mid-write cannot truncate the
/// user's settings.
pub fn write_atomic(path: &Path, value: &JsonValue) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if path.exists() {
        let _ = std::fs::copy(path, with_suffix(path, ".bak"));
    }
    let tmp = with_suffix(path, ".tmp");
    std::fs::write(&tmp, json_util::to_pretty(value))?;
    std::fs::rename(&tmp, path)
}

fn with_suffix(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    std::path::PathBuf::from(s)
}

/// Render a script path for use inside a hook `command` string.
///
/// Two separate things break on Windows without this, where every host runs
/// its hook commands through git-bash (issue #209):
///
/// * `Path::display()` emits `\` separators and the command is handed to bash
///   unquoted, so bash consumes each backslash as an escape —
///   `bash C:\Users\Jesse\.claude\squeez\hooks\pretooluse.sh` reaches the
///   shell as `C:UsersJesse.claudesqueezhookspretooluse.sh` and every hook
///   event fails with `No such file or directory`.
/// * The same backslashes defeat the `/buddy/` substring used to detect an
///   already-registered shim, so `squeez setup` re-appends it on every run.
///
/// Forward slashes are accepted by every Windows API these paths reach, and
/// quoting is inert on Unix apart from making paths with spaces work there
/// too.
pub fn shell_arg(path: &Path) -> String {
    format!("\"{}\"", normalize_sep(&path.display().to_string()))
}

/// `\` → `/`, so a command string can be matched with one separator regardless
/// of which platform wrote it.
pub fn normalize_sep(s: &str) -> String {
    s.replace('\\', "/")
}

/// True if `cmd` points into the vendored buddy directory.
///
/// Deliberately separator-agnostic: settings files written by squeez ≤ 1.46.0
/// on Windows carry `\buddy\`, and those entries must still be recognised so
/// they get upgraded rather than duplicated.
pub fn is_buddy_cmd(cmd: &str) -> bool {
    normalize_sep(cmd).contains("/buddy/")
}

/// True if any hook command inside this matcher entry satisfies `pred`.
fn entry_cmd_any(entry: &JsonValue, pred: impl Fn(&str) -> bool) -> bool {
    entry
        .get("hooks")
        .map(|h| h.as_arr())
        .unwrap_or(&[])
        .iter()
        .any(|h| pred(h.get_str("command")))
}

/// True if the entry list already carries a squeez-core hook.
///
/// Buddy commands live under `.../squeez/buddy/` and must NOT satisfy this —
/// otherwise a lone buddy entry would suppress re-adding missing core hooks.
pub fn has_squeez(entries: &[JsonValue]) -> bool {
    entries
        .iter()
        .any(|m| entry_cmd_any(m, |cmd| cmd.contains("squeez") && !is_buddy_cmd(cmd)))
}

/// True if the entry list already carries a buddy hook.
pub fn has_buddy(entries: &[JsonValue]) -> bool {
    entries.iter().any(|m| entry_cmd_any(m, is_buddy_cmd))
}

/// A hook entry: `{"hooks":[{"type":"command","command":cmd}]}`, plus an
/// optional `"matcher"` placed first to match the previous key ordering.
pub fn hook_entry(matcher: Option<&str>, command: &str) -> JsonValue {
    let mut fields = Vec::new();
    if let Some(m) = matcher {
        fields.push(("matcher".to_string(), JsonValue::Str(m.to_string())));
    }
    fields.push((
        "hooks".to_string(),
        JsonValue::Arr(vec![JsonValue::command_entry(command)]),
    ));
    JsonValue::Obj(fields)
}

/// Append `entry` under `event` unless a squeez-core hook is already there.
pub fn ensure_squeez_hook(hooks_root: &mut JsonValue, event: &str, entry: JsonValue) {
    let arr = hooks_root.ensure_arr(event);
    if !has_squeez(arr) {
        arr.push(entry);
    }
}

/// Register squeez's hook for `event`, rewriting an existing registration of
/// the same `script` in place instead of appending a second copy.
///
/// [`ensure_squeez_hook`] only asks "is *a* squeez hook here?", so once an
/// install exists its command string is frozen forever. That is how the broken
/// Windows registrations in issue #209 survived every `squeez update`: the
/// mangled `bash C:\Users\…\pretooluse.sh` still contains `squeez`, so it was
/// considered present and never corrected. Upgrading in place lets a broken
/// install heal itself on the next `squeez setup`.
pub fn upgrade_squeez_hook(
    hooks_root: &mut JsonValue,
    event: &str,
    script: &str,
    matcher: Option<&str>,
    command: &str,
) {
    upgrade_hook(hooks_root, event, matcher, command, |cmd| {
        is_core_script(cmd, script)
    })
}

/// Register a buddy shim for `event`, rewriting an existing registration of the
/// same shim in place.
///
/// Buddy needs this for the same reason the core hooks do. Making buddy
/// detection separator-agnostic is what lets a Windows-written `\buddy\` entry
/// be RECOGNISED — but recognising it under a presence-only check is precisely
/// what would freeze it: `squeez setup` would see the shim as already present
/// and leave the unrunnable command in place forever.
pub fn upgrade_buddy_hook(
    hooks_root: &mut JsonValue,
    event: &str,
    shim: &str,
    matcher: Option<&str>,
    command: &str,
) {
    upgrade_hook(hooks_root, event, matcher, command, |cmd| {
        is_buddy_cmd(cmd) && normalize_sep(cmd).contains(shim)
    })
}

/// True if `cmd` is squeez's own registration of core hook `script`.
///
/// Anchored on `/hooks/`, and buddy is excluded: the buddy SessionStart shim is
/// also called `session-start.sh`, so an unanchored substring match would
/// convert the buddy registration into a second copy of the core hook.
fn is_core_script(cmd: &str, script: &str) -> bool {
    let norm = normalize_sep(cmd);
    cmd.contains("squeez") && !is_buddy_cmd(cmd) && norm.contains(&format!("/hooks/{script}"))
}

/// Shared body of the two upgraders: rewrite the hook that `matches`, or append.
fn upgrade_hook(
    hooks_root: &mut JsonValue,
    event: &str,
    matcher: Option<&str>,
    command: &str,
    matches: impl Fn(&str) -> bool,
) {
    let arr = hooks_root.ensure_arr(event);
    let Some(entry) = arr.iter_mut().find(|m| entry_cmd_any(m, &matches)) else {
        arr.push(hook_entry(matcher, command));
        return;
    };
    let hooks = entry.get_mut("hooks").and_then(|h| h.as_arr_mut());
    let Some(hooks) = hooks else { return };
    // Rewrite the hook that actually matched — NOT hooks[0]. Another tool may
    // have merged its own hook into this entry, and overwriting index 0 would
    // delete it and leave squeez's hook registered twice.
    let mut only_ours = true;
    for hook in hooks.iter_mut() {
        if !matches(hook.get_str("command")) {
            only_ours = false;
            continue;
        }
        if hook.get_str("command") != command {
            hook.set("command", JsonValue::Str(command.to_string()));
        }
    }
    // The matcher is a property of the whole entry. Widening it when a foreign
    // hook shares the entry would silently change what that hook fires on.
    if let Some(m) = matcher {
        if only_ours && entry.get_str("matcher") != m {
            entry.set("matcher", JsonValue::Str(m.to_string()));
        }
    }
}

/// [`patch_events`]'s upgrader: same shape as [`upgrade_hook`], but replaces
/// the whole matching hook object (not just `command`) so a stale `name` or
/// `timeout` from an older squeez version is corrected too — fields
/// `upgrade_hook` doesn't know about because Claude Code's specs never carry
/// them.
fn upgrade_hook_spec(hooks_root: &mut JsonValue, spec: &HookSpec) {
    let arr = hooks_root.ensure_arr(spec.event);
    let matches = |cmd: &str| is_core_script(cmd, spec.script);
    let Some(entry) = arr.iter_mut().find(|m| entry_cmd_any(m, &matches)) else {
        arr.push(spec.to_entry());
        return;
    };
    let hooks = entry.get_mut("hooks").and_then(|h| h.as_arr_mut());
    let Some(hooks) = hooks else { return };
    let mut only_ours = true;
    for hook in hooks.iter_mut() {
        if !matches(hook.get_str("command")) {
            only_ours = false;
            continue;
        }
        *hook = spec.to_hook();
    }
    if let Some(m) = spec.matcher {
        if only_ours && entry.get_str("matcher") != m {
            entry.set("matcher", JsonValue::Str(m.to_string()));
        }
    }
}

/// Append `entry` under `event` unless a buddy hook is already there.
pub fn ensure_buddy_hook(hooks_root: &mut JsonValue, event: &str, entry: JsonValue) {
    let arr = hooks_root.ensure_arr(event);
    if !has_buddy(arr) {
        arr.push(entry);
    }
}

/// Drop every entry under `event` whose commands match `pred`, removing the
/// event key entirely once it is empty.
pub fn strip_entries(root: &mut JsonValue, event: &str, pred: impl Fn(&str) -> bool + Copy) {
    let Some(arr) = root.get_mut(event).and_then(|v| v.as_arr_mut()) else {
        return;
    };
    arr.retain(|m| !(m.is_obj() && entry_cmd_any(m, pred)));
    if arr.is_empty() {
        root.remove(event);
    }
}

/// Remove squeez's own hook entries for `event` — the uninstall counterpart of
/// [`ensure_squeez_hook`]. Buddy entries are squeez's too, so they go as well.
pub fn strip_squeez(root: &mut JsonValue, event: &str) {
    strip_entries(root, event, |cmd| cmd.contains("squeez"));
}

/// Remove buddy hook entries for `event`, leaving squeez-core hooks in place.
pub fn strip_buddy(root: &mut JsonValue, event: &str) {
    strip_entries(root, event, is_buddy_cmd);
}

/// One hook registration for the simpler adapters (copilot / gemini / codex),
/// which differ only in where the event map lives and which optional keys the
/// hook object carries.
pub struct HookSpec {
    pub event: &'static str,
    pub matcher: Option<&'static str>,
    pub command: String,
    /// Gemini labels its entries; the others omit the key.
    pub name: Option<String>,
    /// Gemini and Codex carry a per-hook timeout; copilot does not.
    pub timeout_ms: Option<u64>,
    /// Bare script filename (e.g. `codex-pretooluse.sh`), used by
    /// [`patch_events`] to find and upgrade a prior registration of this hook
    /// in place — see [`upgrade_squeez_hook`] for why presence-only checks
    /// freeze a broken command string forever.
    pub script: &'static str,
}

impl HookSpec {
    /// The `{"type":"command","command":..}` object, plus `name`/`timeout`
    /// when the host uses them. This is the part [`upgrade_events`] replaces
    /// wholesale on upgrade, so a stale `name`/`timeout` from an older
    /// squeez version can't survive alongside a corrected `command`.
    fn to_hook(&self) -> JsonValue {
        let mut hook = Vec::new();
        if let Some(n) = &self.name {
            hook.push(("name".to_string(), JsonValue::Str(n.clone())));
        }
        hook.push(("type".to_string(), JsonValue::Str("command".to_string())));
        hook.push(("command".to_string(), JsonValue::Str(self.command.clone())));
        if let Some(t) = self.timeout_ms {
            hook.push(("timeout".to_string(), JsonValue::Num(t as f64)));
        }
        JsonValue::Obj(hook)
    }

    fn to_entry(&self) -> JsonValue {
        let mut entry = Vec::new();
        if let Some(m) = self.matcher {
            entry.push(("matcher".to_string(), JsonValue::Str(m.to_string())));
        }
        entry.push((
            "hooks".to_string(),
            JsonValue::Arr(vec![self.to_hook()]),
        ));
        JsonValue::Obj(entry)
    }
}

/// Where a host keeps its event map: at the top level, or nested under
/// `settings["hooks"]`.
#[derive(Clone, Copy, PartialEq)]
pub enum EventRoot {
    TopLevel,
    Nested,
}

/// Register `specs`, appending each only when no squeez hook is present for
/// that event, upgrading a matching prior registration in place instead of
/// freezing it — the same self-heal `upgrade_squeez_hook` gives Claude Code
/// (issue #209: a broken command string still contains "squeez", so a
/// presence-only check never corrects it on later `squeez setup` runs).
/// Idempotent, and never disturbs foreign entries.
pub fn patch_events(
    path: &Path,
    root_kind: EventRoot,
    specs: &[HookSpec],
) -> std::io::Result<()> {
    let mut settings = match load(path) {
        Ok(Existing::Object(v)) => v,
        Ok(Existing::Missing) => JsonValue::Obj(Vec::new()),
        Err(e) => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
    };

    match root_kind {
        EventRoot::TopLevel => {
            for spec in specs {
                upgrade_hook_spec(&mut settings, spec);
            }
        }
        EventRoot::Nested => {
            let root = settings.ensure_obj("hooks");
            for spec in specs {
                upgrade_hook_spec(root, spec);
            }
        }
    }
    write_atomic(path, &settings)
}

/// Reverse [`patch_events`]: drop squeez entries for each event, then drop an
/// emptied nested `hooks` map.
pub fn unpatch_events(
    path: &Path,
    root_kind: EventRoot,
    events: &[&str],
) -> std::io::Result<()> {
    let Some(mut settings) = load_lenient(path) else {
        return Ok(());
    };
    match root_kind {
        EventRoot::TopLevel => {
            for event in events {
                strip_squeez(&mut settings, event);
            }
        }
        EventRoot::Nested => {
            if let Some(root) = settings.get_mut("hooks").filter(|h| h.is_obj()) {
                for event in events {
                    strip_squeez(root, event);
                }
            }
            if settings.get("hooks").is_some_and(|h| h.obj_is_empty()) {
                settings.remove("hooks");
            }
        }
    }
    write_atomic(path, &settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(json: &str) -> JsonValue {
        json_util::parse_value(json).unwrap()
    }

    #[test]
    fn buddy_entry_does_not_satisfy_squeez_check() {
        let v = obj(r#"[{"hooks":[{"command":"bash /x/squeez/buddy/shims/stop.sh"}]}]"#);
        assert!(!has_squeez(v.as_arr()));
        assert!(has_buddy(v.as_arr()));
    }

    #[test]
    fn core_entry_satisfies_squeez_check_only() {
        let v = obj(r#"[{"hooks":[{"command":"bash /x/squeez/hooks/pretooluse.sh"}]}]"#);
        assert!(has_squeez(v.as_arr()));
        assert!(!has_buddy(v.as_arr()));
    }

    #[test]
    fn ensure_hook_is_idempotent() {
        let mut root = obj("{}");
        for _ in 0..3 {
            ensure_squeez_hook(
                &mut root,
                "PreToolUse",
                hook_entry(Some("Bash"), "bash /x/squeez/hooks/pretooluse.sh"),
            );
        }
        assert_eq!(root.get("PreToolUse").unwrap().as_arr().len(), 1);
    }

    #[test]
    fn ensure_arr_replaces_non_array_value() {
        let mut root = obj(r#"{"PreToolUse":"garbage"}"#);
        ensure_squeez_hook(&mut root, "PreToolUse", hook_entry(None, "bash /squeez/x.sh"));
        assert_eq!(root.get("PreToolUse").unwrap().as_arr().len(), 1);
    }

    #[test]
    fn strip_removes_event_key_when_emptied() {
        let mut root = obj(r#"{"Stop":[{"hooks":[{"command":"bash /x/squeez/h.sh"}]}]}"#);
        strip_squeez(&mut root, "Stop");
        assert!(root.get("Stop").is_none());
    }

    #[test]
    fn strip_keeps_foreign_hooks() {
        let mut root = obj(
            r#"{"Stop":[{"hooks":[{"command":"bash /x/squeez/h.sh"}]},
                        {"hooks":[{"command":"bash /other/tool.sh"}]}]}"#,
        );
        strip_squeez(&mut root, "Stop");
        let arr = root.get("Stop").unwrap().as_arr();
        assert_eq!(arr.len(), 1);
        assert!(arr[0].get("hooks").unwrap().as_arr()[0]
            .get_str("command")
            .contains("/other/tool.sh"));
    }

    #[test]
    fn load_rejects_malformed_json_instead_of_overwriting() {
        let dir = std::env::temp_dir().join(format!("squeez_sj_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("settings.json");
        std::fs::write(&p, "{ not json").unwrap();
        assert!(load(&p).is_err());
        assert!(load_lenient(&p).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_strips_utf8_bom() {
        let dir = std::env::temp_dir().join(format!("squeez_sj_bom_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("settings.json");
        std::fs::write(&p, "\u{feff}{\"a\":1}").unwrap();
        assert!(matches!(load(&p), Ok(Existing::Object(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_atomic_backs_up_previous_contents() {
        let dir = std::env::temp_dir().join(format!("squeez_sj_bak_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("settings.json");
        std::fs::write(&p, r#"{"old":true}"#).unwrap();
        write_atomic(&p, &obj(r#"{"new":true}"#)).unwrap();
        assert!(std::fs::read_to_string(&p).unwrap().contains("new"));
        let bak = std::fs::read_to_string(dir.join("settings.json.bak")).unwrap();
        assert!(bak.contains("old"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
