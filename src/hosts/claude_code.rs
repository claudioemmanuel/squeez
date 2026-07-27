//! Claude Code adapter.
//!
//! - install() — drops three hook scripts (PreToolUse / SessionStart /
//!   PostToolUse) + statusline into ~/.claude/squeez/, then patches
//!   ~/.claude/settings.json to register them
//! - inject_memory() — writes the squeez persona block into
//!   ~/.claude/CLAUDE.md (Claude Code auto-loads this at every session)
//! - uninstall() — removes squeez entries from settings.json, removes the
//!   persona block from CLAUDE.md; leaves data_dir intact so users don't
//!   lose session history on reinstall

use std::path::{Path, PathBuf};

use crate::commands::focus;
use crate::commands::persona;
use crate::config::Config;
use crate::json_util::JsonValue;
use crate::memory::Summary;
use crate::session::home_dir;

use super::{memory_size, settings_json, HostAdapter, HostCaps};

const PRETOOLUSE_SCRIPT: &str = include_str!("../../hooks/pretooluse.sh");
const SESSION_START_SCRIPT: &str = include_str!("../../hooks/session-start.sh");
const POSTTOOLUSE_SCRIPT: &str = include_str!("../../hooks/posttooluse.sh");
const SUBAGENT_STOP_SCRIPT: &str = include_str!("../../hooks/subagent-stop.sh");
const PRECOMPACT_SCRIPT: &str = include_str!("../../hooks/precompact.sh");
const POSTCOMPACT_SCRIPT: &str = include_str!("../../hooks/postcompact.sh");
/// `/squeez` slash command — drives `squeez config` in natural language.
const SQUEEZ_COMMAND: &str = include_str!("../../assets/squeez-command.md");
/// `/buddy` slash command — draws the vendored pato-buddy in the conversation.
const BUDDY_COMMAND: &str = include_str!("../../assets/buddy-command.md");

/// Embedded hook sources keyed by installed filename — lets `squeez doctor`
/// detect drift between the running binary and the installed hook scripts.
pub fn hooks_manifest() -> [(&'static str, &'static str); 6] {
    [
        ("pretooluse.sh", PRETOOLUSE_SCRIPT),
        ("session-start.sh", SESSION_START_SCRIPT),
        ("posttooluse.sh", POSTTOOLUSE_SCRIPT),
        ("subagent-stop.sh", SUBAGENT_STOP_SCRIPT),
        ("precompact.sh", PRECOMPACT_SCRIPT),
        ("postcompact.sh", POSTCOMPACT_SCRIPT),
    ]
}

/// Patches ~/.claude/settings.json to register squeez hooks + statusline.
/// Load-merge-write with atomic rename. Idempotent via substring match on
/// "squeez" in existing command strings.
const PRETOOLUSE_MATCHER: &str = "Bash|Read|Grep|Glob|Agent|Task";

/// Events squeez has ever registered at the top level of settings.json.
/// Claude Code expects them under `settings["hooks"]`; early squeez versions
/// wrote them top-level, so install migrates any stragglers.
const SQUEEZ_EVENTS: [&str; 6] = [
    "PreToolUse",
    "SessionStart",
    "PostToolUse",
    "SubagentStop",
    "PreCompact",
    "PostCompact",
];

fn hook_cmd(hooks_dir: &Path, script: &str) -> String {
    format!("bash {}", hooks_dir.join(script).display())
}

/// Every `command` string inside a matcher entry.
fn entry_commands(entry: &JsonValue) -> Vec<&str> {
    entry
        .get("hooks")
        .map(|h| h.as_arr())
        .unwrap_or(&[])
        .iter()
        .map(|h| h.get_str("command"))
        .collect()
}

/// Detach top-level `event` arrays written by older squeez versions, so the
/// nested hooks map can then be mutated in place.
fn take_legacy_events(settings: &mut JsonValue) -> Vec<(&'static str, Vec<JsonValue>)> {
    SQUEEZ_EVENTS
        .iter()
        .filter_map(|event| match settings.remove(event) {
            Some(JsonValue::Arr(items)) => Some((*event, items)),
            _ => None,
        })
        .collect()
}

/// Fold the detached legacy entries into `hooks_root`, skipping any whose
/// command is already registered there.
fn migrate_legacy_events(
    hooks_root: &mut JsonValue,
    legacy_events: Vec<(&'static str, Vec<JsonValue>)>,
) {
    for (event, legacy) in legacy_events {
        let current = hooks_root.ensure_arr(event);
        let existing: Vec<String> = current
            .iter()
            .flat_map(|m| entry_commands(m))
            .map(|c| c.to_string())
            .collect();
        for entry in legacy {
            if !entry.is_obj() {
                continue;
            }
            if entry_commands(&entry)
                .iter()
                .any(|c| existing.iter().any(|e| e == c))
            {
                continue;
            }
            current.push(entry);
        }
    }
}

/// PreToolUse is upgraded in place rather than appended: an older install may
/// carry a narrower matcher (Bash-only) or a stale hook path, and duplicating
/// the entry would run the hook twice.
fn upgrade_pretooluse(hooks_root: &mut JsonValue, command: &str) {
    let arr = hooks_root.ensure_arr("PreToolUse");
    let existing = arr.iter_mut().find(|m| {
        entry_commands(m)
            .iter()
            .any(|c| c.contains("pretooluse.sh") && c.contains("squeez"))
    });
    let Some(entry) = existing else {
        arr.push(settings_json::hook_entry(Some(PRETOOLUSE_MATCHER), command));
        return;
    };
    if entry.get_str("matcher") != PRETOOLUSE_MATCHER {
        entry.set("matcher", JsonValue::Str(PRETOOLUSE_MATCHER.to_string()));
    }
    if let Some(first) = entry
        .get_mut("hooks")
        .and_then(|h| h.as_arr_mut())
        .and_then(|a| a.first_mut())
    {
        if first.get_str("command") != command {
            first.set("command", JsonValue::Str(command.to_string()));
        }
    }
}

/// Statusline chain wrapper written when buddy adopts a third-party HUD:
/// `bash -c 'input=$(cat); echo "$input" | { <HUD>; } 2>/dev/null; echo "$input" | bash <…>/statusline.sh'`
const CHAIN_PREFIX: &str = "bash -c 'input=$(cat); echo \"$input\" | { ";
const CHAIN_MIDDLE: &str = "; } 2>/dev/null; echo \"$input\" | bash ";

/// Recover the adopted HUD command from a squeez chain statusline, so removing
/// squeez does not take a user's third-party HUD down with it.
fn unwrap_chained_hud(cmd: &str) -> Option<&str> {
    let rest = cmd.strip_prefix(CHAIN_PREFIX)?;
    let mid = rest.find(CHAIN_MIDDLE)?;
    let tail = &rest[mid + CHAIN_MIDDLE.len()..];
    if !tail.ends_with("/statusline.sh'") {
        return None;
    }
    Some(&rest[..mid])
}

/// Drop a prior squeez statusLine registration. The squeez status line competes
/// with third-party HUDs (e.g. claude-hud) and surfaces nothing that the memory
/// banner does not. Non-squeez and buddy statusLines are left untouched.
fn strip_squeez_statusline(settings: &mut JsonValue) {
    let Some(status) = settings.get("statusLine") else {
        return;
    };
    if !status.is_obj() {
        return;
    }
    let cmd = status.get_str("command");
    if !cmd.contains("squeez") || cmd.contains("/buddy/") {
        return;
    }
    match unwrap_chained_hud(cmd).map(|s| s.to_string()) {
        Some(inner) => settings.set("statusLine", JsonValue::command_entry(&inner)),
        None => {
            settings.remove("statusLine");
        }
    }
}

fn buddy_shim(buddy_dir: &Path, name: &str) -> String {
    format!("bash {}", buddy_dir.join("shims").join(name).display())
}

fn hud_command_file(buddy_dir: &Path) -> PathBuf {
    buddy_dir.join("hud-command")
}

/// Register the vendored pato-buddy hooks.
fn register_buddy_hooks(hooks_root: &mut JsonValue, buddy_dir: &Path) {
    settings_json::ensure_buddy_hook(
        hooks_root,
        "SessionStart",
        settings_json::hook_entry(None, &buddy_shim(buddy_dir, "session-start.sh")),
    );
    settings_json::ensure_buddy_hook(
        hooks_root,
        "PostToolUse",
        settings_json::hook_entry(
            Some("Edit|Write|Bash"),
            &buddy_shim(buddy_dir, "post-tool-use.sh"),
        ),
    );
    settings_json::ensure_buddy_hook(
        hooks_root,
        "Stop",
        settings_json::hook_entry(None, &buddy_shim(buddy_dir, "stop.sh")),
    );
}

/// Point the statusLine at the buddy chain shim.
///
/// An existing third-party HUD is ADOPTED, not clobbered: its command is stored
/// in `<buddy>/hud-command` and the chain shim keeps rendering it — restored
/// verbatim on `buddy = false` or uninstall.
fn register_buddy_statusline(settings: &mut JsonValue, buddy_dir: &Path) {
    let current = settings
        .get("statusLine")
        .filter(|s| s.is_obj())
        .map(|s| s.get_str("command").to_string())
        .unwrap_or_default();
    if current.contains("/buddy/") {
        return;
    }
    if !current.is_empty() {
        let _ = std::fs::write(hud_command_file(buddy_dir), &current);
    }
    settings.set(
        "statusLine",
        JsonValue::command_entry(&buddy_shim(buddy_dir, "statusline.sh")),
    );
}

/// Restore the statusLine that buddy adopted, or drop it when none was stored.
fn restore_adopted_hud(settings: &mut JsonValue, buddy_dir: &Path) {
    let original = std::fs::read_to_string(hud_command_file(buddy_dir))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if original.is_empty() {
        settings.remove("statusLine");
    } else {
        settings.set("statusLine", JsonValue::command_entry(&original));
    }
}

/// Remove buddy's hooks (the `buddy = false` path). Files stay on disk but
/// inert.
fn unregister_buddy_hooks(hooks_root: &mut JsonValue) {
    for event in ["SessionStart", "PostToolUse", "Stop"] {
        settings_json::strip_buddy(hooks_root, event);
    }
}

/// Hand the statusLine back to whatever buddy adopted.
fn unregister_buddy_statusline(settings: &mut JsonValue, data_dir: &Path) {
    let is_buddy_status = settings
        .get("statusLine")
        .filter(|s| s.is_obj())
        .is_some_and(|s| s.get_str("command").contains("/buddy/"));
    if is_buddy_status {
        restore_adopted_hud(settings, &data_dir.join("buddy"));
    }
}

/// Register every squeez hook in the host settings file.
///
/// Pure Rust: this used to shell out to `python3`, which made setup fail on
/// machines without a `python3` on PATH (issue #190).
fn patch_settings(
    settings_path: &Path,
    hooks_dir: &Path,
    data_dir: &Path,
    buddy_dir: Option<&Path>,
) -> std::io::Result<()> {
    let mut settings = match settings_json::load(settings_path) {
        Ok(settings_json::Existing::Object(v)) => v,
        Ok(settings_json::Existing::Missing) => JsonValue::Obj(Vec::new()),
        Err(e) => {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e));
        }
    };

    // statusLine lives at the top level; settle it before borrowing the
    // nested hooks map. Buddy adopts whatever survives the squeez strip.
    strip_squeez_statusline(&mut settings);
    match buddy_dir {
        Some(dir) => register_buddy_statusline(&mut settings, dir),
        None => unregister_buddy_statusline(&mut settings, data_dir),
    }

    // Claude Code expects hooks at settings["hooks"][event], not top-level.
    let legacy = take_legacy_events(&mut settings);
    let hooks_root = settings.ensure_obj("hooks");
    migrate_legacy_events(hooks_root, legacy);

    upgrade_pretooluse(hooks_root, &hook_cmd(hooks_dir, "pretooluse.sh"));
    for (event, script) in [
        ("SessionStart", "session-start.sh"),
        ("PostToolUse", "posttooluse.sh"),
        ("SubagentStop", "subagent-stop.sh"),
        ("PreCompact", "precompact.sh"),
        ("PostCompact", "postcompact.sh"),
    ] {
        settings_json::ensure_squeez_hook(
            hooks_root,
            event,
            settings_json::hook_entry(None, &hook_cmd(hooks_dir, script)),
        );
    }

    match buddy_dir {
        Some(dir) => register_buddy_hooks(hooks_root, dir),
        None => unregister_buddy_hooks(hooks_root),
    }

    settings_json::write_atomic(settings_path, &settings)
}

/// Reverse [`patch_settings`], leaving every non-squeez entry untouched.
fn unpatch_settings(settings_path: &Path) -> std::io::Result<()> {
    let Some(mut settings) = settings_json::load_lenient(settings_path) else {
        return Ok(());
    };

    let events: Vec<&str> = SQUEEZ_EVENTS
        .iter()
        .copied()
        .chain(std::iter::once("Stop"))
        .collect();
    if let Some(root) = settings.get_mut("hooks").filter(|h| h.is_obj()) {
        for event in &events {
            settings_json::strip_squeez(root, event);
        }
    }
    if settings.get("hooks").is_some_and(|h| h.obj_is_empty()) {
        settings.remove("hooks");
    }
    // Legacy shape: settings[event] (pre-fix installs).
    for event in &events {
        settings_json::strip_squeez(&mut settings, event);
    }

    let status_cmd = settings
        .get("statusLine")
        .filter(|s| s.is_obj())
        .map(|s| s.get_str("command").to_string())
        .unwrap_or_default();
    if let Some(buddy_dir) = status_cmd
        .strip_prefix("bash ")
        .and_then(|c| c.split_once("/shims/statusline.sh"))
        .map(|(dir, _)| PathBuf::from(dir))
        .filter(|_| status_cmd.contains("/buddy/"))
    {
        restore_adopted_hud(&mut settings, &buddy_dir);
    } else if status_cmd.contains("/buddy/") || status_cmd.contains("squeez") {
        settings.remove("statusLine");
    }

    settings_json::write_atomic(settings_path, &settings)
}

pub struct ClaudeCodeAdapter;

impl ClaudeCodeAdapter {
    fn claude_dir() -> PathBuf {
        PathBuf::from(format!("{}/.claude", home_dir()))
    }

    fn settings_path() -> PathBuf {
        Self::claude_dir().join("settings.json")
    }

    fn claude_md_path() -> PathBuf {
        Self::claude_dir().join("CLAUDE.md")
    }

    /// `~/.claude/commands/squeez.md` — the user-defined `/squeez` slash command.
    fn command_path() -> PathBuf {
        Self::claude_dir().join("commands").join("squeez.md")
    }

    /// `~/.claude/commands/buddy.md` — the `/buddy` slash command (vendored duck).
    fn buddy_command_path() -> PathBuf {
        Self::claude_dir().join("commands").join("buddy.md")
    }
}

fn hooks_dir_for(data_dir: &Path) -> PathBuf {
    data_dir.join("hooks")
}

fn bin_dir_for(data_dir: &Path) -> PathBuf {
    data_dir.join("bin")
}

fn write_hook(dir: &Path, name: &str, body: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(name);
    std::fs::write(&path, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

impl HostAdapter for ClaudeCodeAdapter {
    fn name(&self) -> &'static str {
        "claude-code"
    }

    fn is_installed(&self) -> bool {
        Self::claude_dir().exists()
    }

    fn data_dir(&self) -> PathBuf {
        std::env::var("SQUEEZ_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| Self::claude_dir().join("squeez"))
    }

    fn capabilities(&self) -> HostCaps {
        HostCaps::BASH_WRAP | HostCaps::SESSION_MEM | HostCaps::BUDGET_HARD
    }

    fn install(&self, _bin_path: &Path) -> std::io::Result<()> {
        let data = self.data_dir();
        let hooks = hooks_dir_for(&data);
        let bin = bin_dir_for(&data);
        std::fs::create_dir_all(&hooks)?;
        std::fs::create_dir_all(&bin)?;
        std::fs::create_dir_all(data.join("sessions"))?;
        std::fs::create_dir_all(data.join("memory"))?;

        write_hook(&hooks, "pretooluse.sh", PRETOOLUSE_SCRIPT)?;
        write_hook(&hooks, "session-start.sh", SESSION_START_SCRIPT)?;
        write_hook(&hooks, "posttooluse.sh", POSTTOOLUSE_SCRIPT)?;
        write_hook(&hooks, "subagent-stop.sh", SUBAGENT_STOP_SCRIPT)?;
        write_hook(&hooks, "precompact.sh", PRECOMPACT_SCRIPT)?;
        write_hook(&hooks, "postcompact.sh", POSTCOMPACT_SCRIPT)?;

        // Remove orphan statusline.sh artifacts left by earlier installs. The
        // status line is no longer registered (see PATCH_SCRIPT) and the
        // script file would otherwise linger across upgrades.
        for orphan in [bin.join("statusline.sh"), hooks.join("statusline.sh")] {
            let _ = std::fs::remove_file(&orphan);
        }

        // Vendored pato-buddy: materialized + registered by default; when
        // `buddy = false` the entries are stripped instead (files stay inert).
        let buddy_dir = if crate::config::Config::load().buddy {
            Some(super::buddy::materialize(&data)?)
        } else {
            None
        };

        patch_settings(
            &Self::settings_path(),
            &hooks,
            &data,
            buddy_dir.as_deref(),
        )?;

        // Install the `/squeez` slash command.
        let cmd_path = Self::command_path();
        if let Some(parent) = cmd_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&cmd_path, SQUEEZ_COMMAND)?;

        // `/buddy` follows the buddy toggle: installed with it, removed with it.
        let buddy_cmd = Self::buddy_command_path();
        if buddy_dir.is_none() {
            let _ = std::fs::remove_file(&buddy_cmd);
        } else {
            std::fs::write(&buddy_cmd, BUDDY_COMMAND)?;
        }
        Ok(())
    }

    fn uninstall(&self) -> std::io::Result<()> {
        let settings = Self::settings_path();
        if settings.exists() {
            unpatch_settings(&settings)?;
        }
        let claude_md = Self::claude_md_path();
        if claude_md.exists() {
            let existing = std::fs::read_to_string(&claude_md).unwrap_or_default();
            let cleaned = strip_squeez_block(&existing);
            let _ = std::fs::write(&claude_md, cleaned);
        }
        // Remove the `/squeez` and `/buddy` slash commands we installed.
        let _ = std::fs::remove_file(Self::command_path());
        let _ = std::fs::remove_file(Self::buddy_command_path());
        Ok(())
    }

    /// Writes the squeez persona block into `~/.claude/CLAUDE.md`.
    fn inject_memory(&self, cfg: &Config, _summaries: &[Summary]) -> std::io::Result<()> {
        let home = home_dir();
        let claude_dir = format!("{}/.claude", home);
        let path = format!("{}/CLAUDE.md", claude_dir);
        std::fs::create_dir_all(&claude_dir)?;

        let persona_text = persona::text_with_lang(cfg.persona, &cfg.lang);
        // persona=off + focus=off means there is nothing to inject at all;
        // focus alone is still worth a block.
        if persona_text.is_empty() && cfg.focus == focus::Focus::Off {
            return Ok(());
        }

        let existing = std::fs::read_to_string(&path).unwrap_or_default();

        let mut block = String::from("<!-- squeez:start -->\n");
        if let Some(banner) =
            memory_size::size_warning(&existing, "CLAUDE.md", cfg.memory_file_warn_tokens)
        {
            block.push_str(&banner);
        }
        block.push_str("## squeez — always-on compression\n\n");
        block.push_str(&format!(
            "Persona: {}{} | Bash compression: ON | Memory: ON\n\n",
            persona::as_str(cfg.persona),
            focus::banner_suffix(cfg.focus)
        ));
        block.push_str(persona_text);
        if !persona_text.ends_with('\n') {
            block.push('\n');
        }
        let focus_text = focus::text_with_lang(cfg.focus, &cfg.lang);
        if !focus_text.is_empty() {
            block.push('\n');
            block.push_str(focus_text);
            if !focus_text.ends_with('\n') {
                block.push('\n');
            }
        }
        block.push_str("<!-- squeez:end -->\n");

        let cleaned = strip_squeez_block(&existing);
        let contents = format!("{}\n{}", block, cleaned.trim_start());
        std::fs::write(&path, contents)
    }
}

fn strip_squeez_block(s: &str) -> String {
    if !s.contains("<!-- squeez:start -->") {
        return s.to_string();
    }
    let start = s.find("<!-- squeez:start -->").unwrap_or(0);
    let end = s
        .find("<!-- squeez:end -->")
        .map(|i| i + "<!-- squeez:end -->".len() + 1)
        .unwrap_or(start);
    format!("{}{}", &s[..start], &s[end.min(s.len())..])
}
