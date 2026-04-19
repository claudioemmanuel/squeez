//! Copilot CLI adapter.

use std::path::{Path, PathBuf};

use crate::commands::persona;
use crate::config::Config;
use crate::memory::Summary;
use crate::session::home_dir;

use super::{memory_size, HostAdapter, HostCaps};

const PRETOOLUSE_SCRIPT: &str = include_str!("../../hooks/copilot-pretooluse.sh");
const SESSION_START_SCRIPT: &str = include_str!("../../hooks/copilot-session-start.sh");
const POSTTOOLUSE_SCRIPT: &str = include_str!("../../hooks/copilot-posttooluse.sh");

const PATCH_SCRIPT: &str = r#"
import json, os, sys

path = sys.argv[1]
hooks_dir = sys.argv[2]

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
        "hooks": [{"type": "command", "command": "bash " + os.path.join(hooks_dir, "copilot-pretooluse.sh")}],
    })

ensure_list("SessionStart")
if not has_squeez(settings["SessionStart"]):
    settings["SessionStart"].append({
        "hooks": [{"type": "command", "command": "bash " + os.path.join(hooks_dir, "copilot-session-start.sh")}],
    })

ensure_list("PostToolUse")
if not has_squeez(settings["PostToolUse"]):
    settings["PostToolUse"].append({
        "hooks": [{"type": "command", "command": "bash " + os.path.join(hooks_dir, "copilot-posttooluse.sh")}],
    })

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

tmp = path + ".tmp"
with open(tmp, "w") as f:
    json.dump(settings, f, indent=2)
os.replace(tmp, path)
"#;

pub struct CopilotCliAdapter;

impl CopilotCliAdapter {
    fn copilot_dir() -> PathBuf {
        PathBuf::from(format!("{}/.copilot", home_dir()))
    }
    fn settings_path() -> PathBuf {
        Self::copilot_dir().join("settings.json")
    }
    fn instructions_path() -> PathBuf {
        Self::copilot_dir().join("copilot-instructions.md")
    }
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

impl HostAdapter for CopilotCliAdapter {
    fn name(&self) -> &'static str {
        "copilot"
    }

    fn is_installed(&self) -> bool {
        Self::copilot_dir().exists()
    }

    fn data_dir(&self) -> PathBuf {
        std::env::var("SQUEEZ_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| Self::copilot_dir().join("squeez"))
    }

    fn capabilities(&self) -> HostCaps {
        HostCaps::BASH_WRAP | HostCaps::SESSION_MEM | HostCaps::BUDGET_HARD
    }

    fn install(&self, _bin_path: &Path) -> std::io::Result<()> {
        let data = self.data_dir();
        let hooks = data.join("hooks");
        std::fs::create_dir_all(&hooks)?;
        std::fs::create_dir_all(data.join("sessions"))?;
        std::fs::create_dir_all(data.join("memory"))?;

        write_hook(&hooks, "copilot-pretooluse.sh", PRETOOLUSE_SCRIPT)?;
        write_hook(&hooks, "copilot-session-start.sh", SESSION_START_SCRIPT)?;
        write_hook(&hooks, "copilot-posttooluse.sh", POSTTOOLUSE_SCRIPT)?;

        run_python(
            PATCH_SCRIPT,
            &[
                Self::settings_path().to_str().unwrap_or(""),
                hooks.to_str().unwrap_or(""),
            ],
        )?;
        Ok(())
    }

    fn uninstall(&self) -> std::io::Result<()> {
        let settings = Self::settings_path();
        if settings.exists() {
            run_python(UNPATCH_SCRIPT, &[settings.to_str().unwrap_or("")])?;
        }
        let instructions = Self::instructions_path();
        if instructions.exists() {
            let existing = std::fs::read_to_string(&instructions).unwrap_or_default();
            let cleaned = strip_squeez_block(&existing);
            let _ = std::fs::write(&instructions, cleaned);
        }
        Ok(())
    }

    fn inject_memory(&self, cfg: &Config, summaries: &[Summary]) -> std::io::Result<()> {
        let path = Self::instructions_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let existing = std::fs::read_to_string(&path).unwrap_or_default();

        let mut block = String::from("<!-- squeez:start -->\n");
        if let Some(banner) = memory_size::size_warning(
            &existing,
            "copilot-instructions.md",
            cfg.memory_file_warn_tokens,
        ) {
            block.push_str(&banner);
        }
        block.push_str("## squeez — session context\n");
        let budget_k = cfg.compact_threshold_tokens * 5 / 4 / 1000;
        block.push_str(&format!(
            "Context budget: ~{}K tokens | Compression: ON | Memory: ON | Persona: {}\n",
            budget_k,
            persona::as_str(cfg.persona)
        ));
        for s in summaries {
            block.push_str(&format!("- {}\n", s.display_line()));
        }
        if summaries.is_empty() {
            block.push_str("- No prior sessions recorded yet.\n");
        }
        let persona_text = persona::text_with_lang(cfg.persona, &cfg.lang);
        if !persona_text.is_empty() {
            block.push('\n');
            block.push_str(persona_text);
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
