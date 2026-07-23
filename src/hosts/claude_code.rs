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
use crate::memory::Summary;
use crate::session::home_dir;

use super::{memory_size, HostAdapter, HostCaps};

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
const PATCH_SCRIPT: &str = r#"
import json, os, shutil, sys

path = sys.argv[1]
hooks_dir = sys.argv[2]
statusline_bin = sys.argv[3]
# Buddy root dir ("" = buddy disabled — strip its entries instead of adding).
buddy_dir = sys.argv[4] if len(sys.argv) > 4 else ""

settings = {}
file_existed = os.path.exists(path)
if file_existed:
    try:
        with open(path, "r", encoding="utf-8-sig") as f:
            settings = json.load(f)
    except Exception as e:
        sys.stderr.write(
            "squeez: refusing to overwrite {path}: could not parse existing JSON ({err}).\n"
            "squeez: fix or remove the file, then re-run `squeez setup`.\n".format(path=path, err=e)
        )
        sys.exit(2)
if not isinstance(settings, dict):
    sys.stderr.write(
        "squeez: refusing to overwrite {path}: top-level value is not a JSON object.\n".format(path=path)
    )
    sys.exit(2)

# Claude Code expects hooks at settings["hooks"][event], not top-level.
if not isinstance(settings.get("hooks"), dict):
    settings["hooks"] = {}
hooks_root = settings["hooks"]

def ensure_list(key):
    if not isinstance(hooks_root.get(key), list):
        hooks_root[key] = []

# Migrate legacy top-level entries written by earlier squeez versions.
for _legacy_evt in ("PreToolUse", "SessionStart", "PostToolUse", "SubagentStop", "PreCompact", "PostCompact"):
    _legacy = settings.pop(_legacy_evt, None)
    if isinstance(_legacy, list):
        ensure_list(_legacy_evt)
        _existing_cmds = {
            str(h.get("command", ""))
            for m in hooks_root[_legacy_evt] if isinstance(m, dict)
            for h in (m.get("hooks") or [])
        }
        for _m in _legacy:
            if not isinstance(_m, dict):
                continue
            _cmds = [str(h.get("command", "")) for h in (_m.get("hooks") or [])]
            if any(c in _existing_cmds for c in _cmds):
                continue
            hooks_root[_legacy_evt].append(_m)

PRETOOLUSE_MATCHER = "Bash|Read|Grep|Glob|Agent|Task"
PRETOOLUSE_CMD = "bash " + os.path.join(hooks_dir, "pretooluse.sh")

def has_squeez(arr):
    # Buddy commands live under .../squeez/buddy/ — they must not satisfy the
    # squeez-core idempotency check, or missing core hooks would never be
    # re-added while a buddy entry exists.
    for m in arr:
        try:
            for h in m.get("hooks", []):
                cmd = str(h.get("command", ""))
                if "squeez" in cmd and "/buddy/" not in cmd:
                    return True
        except Exception:
            continue
    return False

def has_buddy(arr):
    for m in arr:
        try:
            for h in m.get("hooks", []):
                if "/buddy/" in str(h.get("command", "")):
                    return True
        except Exception:
            continue
    return False

def find_squeez_pretooluse(arr):
    """Return the index of an existing squeez PreToolUse entry, or -1."""
    for i, m in enumerate(arr):
        try:
            for h in m.get("hooks", []):
                if "pretooluse.sh" in str(h.get("command", "")) and "squeez" in str(h.get("command", "")):
                    return i
        except Exception:
            continue
    return -1

ensure_list("PreToolUse")
idx = find_squeez_pretooluse(hooks_root["PreToolUse"])
if idx == -1:
    hooks_root["PreToolUse"].append({
        "matcher": PRETOOLUSE_MATCHER,
        "hooks": [{"type": "command", "command": PRETOOLUSE_CMD}],
    })
else:
    entry = hooks_root["PreToolUse"][idx]
    if entry.get("matcher") != PRETOOLUSE_MATCHER:
        entry["matcher"] = PRETOOLUSE_MATCHER
    cmd_list = entry.get("hooks", [])
    if cmd_list and cmd_list[0].get("command") != PRETOOLUSE_CMD:
        cmd_list[0]["command"] = PRETOOLUSE_CMD

ensure_list("SessionStart")
if not has_squeez(hooks_root["SessionStart"]):
    hooks_root["SessionStart"].append({
        "hooks": [{"type": "command", "command": "bash " + os.path.join(hooks_dir, "session-start.sh")}],
    })

ensure_list("PostToolUse")
if not has_squeez(hooks_root["PostToolUse"]):
    hooks_root["PostToolUse"].append({
        "hooks": [{"type": "command", "command": "bash " + os.path.join(hooks_dir, "posttooluse.sh")}],
    })

ensure_list("SubagentStop")
if not has_squeez(hooks_root["SubagentStop"]):
    hooks_root["SubagentStop"].append({
        "hooks": [{"type": "command", "command": "bash " + os.path.join(hooks_dir, "subagent-stop.sh")}],
    })

ensure_list("PreCompact")
if not has_squeez(hooks_root["PreCompact"]):
    hooks_root["PreCompact"].append({
        "hooks": [{"type": "command", "command": "bash " + os.path.join(hooks_dir, "precompact.sh")}],
    })

ensure_list("PostCompact")
if not has_squeez(hooks_root["PostCompact"]):
    hooks_root["PostCompact"].append({
        "hooks": [{"type": "command", "command": "bash " + os.path.join(hooks_dir, "postcompact.sh")}],
    })

# Strip any prior squeez statusLine registration. The squeez status line
# competes with third-party HUDs (e.g. claude-hud) and provides no critical
# information that isn't already surfaced via memory/banner. Leave non-squeez
# statusLine entries untouched. The `statusline_bin` arg is retained for
# backward compatibility with older installers but is no longer consumed.
_ = statusline_bin
import re as _re
existing_status = settings.get("statusLine")
if isinstance(existing_status, dict):
    existing_cmd = str(existing_status.get("command", ""))
    # The buddy statusline (".../squeez/buddy/...") is exempt from the strip.
    if "squeez" in existing_cmd and "/buddy/" not in existing_cmd:
        m = _re.match(r"^bash -c 'input=\$\(cat\); echo \"\$input\" \| \{ (.+); \} 2>/dev/null; echo \"\$input\" \| bash .+/statusline\.sh'$", existing_cmd)
        if m:
            settings["statusLine"] = {"type": "command", "command": m.group(1)}
        else:
            del settings["statusLine"]

# Buddy (vendored pato-buddy): registered by default, stripped when disabled.
if buddy_dir:
    def _buddy_cmd(name):
        return "bash " + os.path.join(buddy_dir, "shims", name)
    ensure_list("SessionStart")
    if not has_buddy(hooks_root["SessionStart"]):
        hooks_root["SessionStart"].append({
            "hooks": [{"type": "command", "command": _buddy_cmd("session-start.sh")}],
        })
    ensure_list("PostToolUse")
    if not has_buddy(hooks_root["PostToolUse"]):
        hooks_root["PostToolUse"].append({
            "matcher": "Edit|Write|Bash",
            "hooks": [{"type": "command", "command": _buddy_cmd("post-tool-use.sh")}],
        })
    ensure_list("Stop")
    if not has_buddy(hooks_root["Stop"]):
        hooks_root["Stop"].append({
            "hooks": [{"type": "command", "command": _buddy_cmd("stop.sh")}],
        })
    # statusLine: the duck rides the end of the HUD's first line. An existing
    # third-party HUD (e.g. claude-hud) is ADOPTED, not clobbered: its command
    # is stored in <buddy>/hud-command and the chain shim keeps rendering it —
    # restored verbatim on buddy=off / uninstall.
    _chain_cmd = _buddy_cmd("statusline.sh")
    _st = settings.get("statusLine")
    _cur = str(_st.get("command", "")) if isinstance(_st, dict) else ""
    if "/buddy/" not in _cur:
        if _cur:
            try:
                with open(os.path.join(buddy_dir, "hud-command"), "w", encoding="utf-8") as _f:
                    _f.write(_cur)
            except Exception:
                pass
        settings["statusLine"] = {"type": "command", "command": _chain_cmd}
else:
    for _evt in ("SessionStart", "PostToolUse", "Stop"):
        _arr = hooks_root.get(_evt)
        if isinstance(_arr, list):
            hooks_root[_evt] = [
                m for m in _arr
                if not (isinstance(m, dict) and any(
                    "/buddy/" in str(h.get("command", "")) for h in (m.get("hooks") or [])
                ))
            ]
            if not hooks_root[_evt]:
                del hooks_root[_evt]
    _st = settings.get("statusLine")
    if isinstance(_st, dict) and "/buddy/" in str(_st.get("command", "")):
        # Restore the adopted HUD command, if one was stored at adoption time.
        _orig = ""
        try:
            _data = os.path.dirname(hooks_dir.rstrip("/"))
            with open(os.path.join(_data, "buddy", "hud-command"), "r", encoding="utf-8") as _f:
                _orig = _f.read().strip()
        except Exception:
            pass
        if _orig:
            settings["statusLine"] = {"type": "command", "command": _orig}
        else:
            del settings["statusLine"]

os.makedirs(os.path.dirname(path), exist_ok=True)
if file_existed:
    try:
        shutil.copy2(path, path + ".bak")
    except Exception:
        pass
tmp = path + ".tmp"
with open(tmp, "w", encoding="utf-8") as f:
    json.dump(settings, f, indent=2)
os.replace(tmp, path)
"#;

const UNPATCH_SCRIPT: &str = r#"
import json, os, shutil, sys

path = sys.argv[1]
if not os.path.exists(path):
    sys.exit(0)
try:
    with open(path, "r", encoding="utf-8-sig") as f:
        settings = json.load(f)
except Exception as e:
    sys.stderr.write(
        "squeez: refusing to rewrite {path}: could not parse existing JSON ({err}).\n".format(path=path, err=e)
    )
    sys.exit(0)
if not isinstance(settings, dict):
    sys.exit(0)

def _strip_squeez(arr):
    return [
        m for m in arr
        if isinstance(m, dict)
        and not any("squeez" in str(h.get("command", "")) for h in (m.get("hooks") or []))
    ]

hooks_root = settings.get("hooks") if isinstance(settings.get("hooks"), dict) else None
for event in ("PreToolUse", "SessionStart", "PostToolUse", "SubagentStop", "PreCompact", "PostCompact", "Stop"):
    # Current shape: settings["hooks"][event]
    if hooks_root is not None:
        arr = hooks_root.get(event)
        if isinstance(arr, list):
            hooks_root[event] = _strip_squeez(arr)
            if not hooks_root[event]:
                del hooks_root[event]
    # Legacy shape: settings[event] (pre-fix installs)
    arr = settings.get(event)
    if isinstance(arr, list):
        settings[event] = _strip_squeez(arr)
        if not settings[event]:
            del settings[event]
if hooks_root is not None and not hooks_root:
    del settings["hooks"]

status = settings.get("statusLine")
if isinstance(status, dict):
    _cmd = str(status.get("command", ""))
    if "/buddy/" in _cmd:
        # Buddy chain: restore the adopted HUD command stored at adoption time.
        import re as _re
        _m = _re.search(r"bash (.*/buddy)/shims/statusline\.sh", _cmd)
        _orig = ""
        if _m:
            try:
                with open(os.path.join(_m.group(1), "hud-command"), "r", encoding="utf-8") as _f:
                    _orig = _f.read().strip()
            except Exception:
                pass
        if _orig:
            settings["statusLine"] = {"type": "command", "command": _orig}
        else:
            del settings["statusLine"]
    elif "squeez" in _cmd:
        del settings["statusLine"]

try:
    shutil.copy2(path, path + ".bak")
except Exception:
    pass
tmp = path + ".tmp"
with open(tmp, "w", encoding="utf-8") as f:
    json.dump(settings, f, indent=2)
os.replace(tmp, path)
"#;

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

fn run_python(script: &str, args: &[&str]) -> std::io::Result<()> {
    let status = std::process::Command::new("python3")
        .arg("-c")
        .arg(script)
        .args(args)
        .status()?;
    if !status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("python3 settings patch exited {status}"),
        ));
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
            super::buddy::materialize(&data)?
                .to_str()
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        };

        run_python(
            PATCH_SCRIPT,
            &[
                Self::settings_path().to_str().unwrap_or(""),
                hooks.to_str().unwrap_or(""),
                "", // legacy positional arg (was statusline_bin); kept for compat
                &buddy_dir,
            ],
        )?;

        // Install the `/squeez` slash command.
        let cmd_path = Self::command_path();
        if let Some(parent) = cmd_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&cmd_path, SQUEEZ_COMMAND)?;

        // `/buddy` follows the buddy toggle: installed with it, removed with it.
        let buddy_cmd = Self::buddy_command_path();
        if buddy_dir.is_empty() {
            let _ = std::fs::remove_file(&buddy_cmd);
        } else {
            std::fs::write(&buddy_cmd, BUDDY_COMMAND)?;
        }
        Ok(())
    }

    fn uninstall(&self) -> std::io::Result<()> {
        let settings = Self::settings_path();
        if settings.exists() {
            run_python(UNPATCH_SCRIPT, &[settings.to_str().unwrap_or("")])?;
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
