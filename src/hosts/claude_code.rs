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

use crate::commands::persona;
use crate::config::Config;
use crate::memory::Summary;
use crate::session::home_dir;

use super::{memory_size, HostAdapter, HostCaps};

const PRETOOLUSE_SCRIPT: &str = include_str!("../../hooks/pretooluse.sh");
const SESSION_START_SCRIPT: &str = include_str!("../../hooks/session-start.sh");
const POSTTOOLUSE_SCRIPT: &str = include_str!("../../hooks/posttooluse.sh");
const STATUSLINE_SCRIPT: &str = include_str!("../../hooks/statusline.sh");

/// Patches ~/.claude/settings.json to register squeez hooks + statusline.
/// Load-merge-write with atomic rename. Idempotent via substring match on
/// "squeez" in existing command strings.
const PATCH_SCRIPT: &str = r#"
import json, os, sys

path = sys.argv[1]
hooks_dir = sys.argv[2]
statusline_bin = sys.argv[3]

settings = {}
if os.path.exists(path):
    try:
        with open(path) as f:
            settings = json.load(f)
    except Exception:
        settings = {}

def ensure_list(key):
    if not isinstance(settings.get(key), list):
        settings[key] = []

def has_squeez(arr):
    for m in arr:
        try:
            for h in m.get("hooks", []):
                if "squeez" in str(h.get("command", "")):
                    return True
        except Exception:
            continue
    return False

ensure_list("PreToolUse")
if not has_squeez(settings["PreToolUse"]):
    settings["PreToolUse"].append({
        "matcher": "Bash",
        "hooks": [{"type": "command", "command": "bash " + os.path.join(hooks_dir, "pretooluse.sh")}],
    })

ensure_list("SessionStart")
if not has_squeez(settings["SessionStart"]):
    settings["SessionStart"].append({
        "hooks": [{"type": "command", "command": "bash " + os.path.join(hooks_dir, "session-start.sh")}],
    })

ensure_list("PostToolUse")
if not has_squeez(settings["PostToolUse"]):
    settings["PostToolUse"].append({
        "hooks": [{"type": "command", "command": "bash " + os.path.join(hooks_dir, "posttooluse.sh")}],
    })

existing_status = settings.get("statusLine")
existing_cmd = existing_status.get("command", "") if isinstance(existing_status, dict) else ""
squeez_cmd = "bash " + statusline_bin
if "squeez" not in existing_cmd:
    if existing_cmd:
        new_cmd = (
            "bash -c 'input=$(cat); echo \"$input\" | { "
            + existing_cmd.rstrip() + "; } 2>/dev/null; echo \"$input\" | "
            + squeez_cmd + "'"
        )
        settings["statusLine"] = {"type": "command", "command": new_cmd}
    else:
        settings["statusLine"] = {"type": "command", "command": squeez_cmd}

os.makedirs(os.path.dirname(path), exist_ok=True)
tmp = path + ".tmp"
with open(tmp, "w") as f:
    json.dump(settings, f, indent=2)
os.replace(tmp, path)
"#;

const UNPATCH_SCRIPT: &str = r#"
import json, os, sys

path = sys.argv[1]
if not os.path.exists(path):
    sys.exit(0)
try:
    with open(path) as f:
        settings = json.load(f)
except Exception:
    sys.exit(0)

for event in ("PreToolUse", "SessionStart", "PostToolUse"):
    arr = settings.get(event)
    if isinstance(arr, list):
        settings[event] = [
            m for m in arr
            if not any("squeez" in str(h.get("command", "")) for h in m.get("hooks", []))
        ]
        if not settings[event]:
            del settings[event]

status = settings.get("statusLine")
if isinstance(status, dict) and "squeez" in str(status.get("command", "")):
    del settings["statusLine"]

tmp = path + ".tmp"
with open(tmp, "w") as f:
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
        write_hook(&bin, "statusline.sh", STATUSLINE_SCRIPT)?;

        run_python(
            PATCH_SCRIPT,
            &[
                Self::settings_path().to_str().unwrap_or(""),
                hooks.to_str().unwrap_or(""),
                bin.join("statusline.sh").to_str().unwrap_or(""),
            ],
        )?;
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
        Ok(())
    }

    /// Writes the squeez persona block into `~/.claude/CLAUDE.md`.
    fn inject_memory(&self, cfg: &Config, _summaries: &[Summary]) -> std::io::Result<()> {
        let home = home_dir();
        let claude_dir = format!("{}/.claude", home);
        let path = format!("{}/CLAUDE.md", claude_dir);
        std::fs::create_dir_all(&claude_dir)?;

        let persona_text = persona::text_with_lang(cfg.persona, &cfg.lang);
        if persona_text.is_empty() {
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
            "Persona: {} | Bash compression: ON | Memory: ON\n\n",
            persona::as_str(cfg.persona)
        ));
        block.push_str(persona_text);
        if !persona_text.ends_with('\n') {
            block.push('\n');
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
