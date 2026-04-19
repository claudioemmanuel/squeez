//! Gemini CLI adapter.
//!
//! Wiring per the Gemini CLI hook system introduced in v0.26.0:
//!   - Config file: `~/.gemini/settings.json`
//!   - Hooks namespace: `hooks.{SessionStart,BeforeTool,AfterTool}` — same
//!     array-of-matcher shape Claude Code uses
//!   - Memory file: `~/.gemini/GEMINI.md` (auto-loaded at session start)
//!
//! Capabilities ship as `BASH_WRAP | SESSION_MEM | BUDGET_SOFT`. The
//! `BeforeTool` docs mention a rewrite capability for tool_input, but the
//! stdout schema is not yet publicly documented, so this adapter treats
//! Read/Grep/Glob budget enforcement as soft (hint in GEMINI.md) until we
//! can empirically confirm the rewrite format. Upstream tracking:
//!   https://github.com/google-gemini/gemini-cli/issues/25629
//!
//! JSON patching of `settings.json` delegates to a python3 subprocess. The
//! binary already requires python3 for Claude Code and Copilot CLI hook
//! scripts, so this is an existing runtime dependency, not a new one.

use std::path::{Path, PathBuf};

use crate::commands::persona;
use crate::config::Config;
use crate::memory::Summary;
use crate::session::home_dir;

use super::{memory_size, HostAdapter, HostCaps};

const SESSION_START_SCRIPT: &str = include_str!("../../hooks/gemini-session-start.sh");
const BEFORE_TOOL_SCRIPT: &str = include_str!("../../hooks/gemini-before-tool.sh");
const AFTER_TOOL_SCRIPT: &str = include_str!("../../hooks/gemini-after-tool.sh");

/// Python program used to patch `~/.gemini/settings.json`. Receives the
/// target path as argv[1] and the squeez-hooks directory as argv[2].
///
/// The script:
/// 1. Loads the existing settings (tolerating missing/malformed files)
/// 2. Ensures `hooks.{SessionStart,BeforeTool,AfterTool}` arrays exist
/// 3. Appends squeez matcher objects if no entry already contains "squeez"
///    in any of its command strings (idempotent on repeated runs)
/// 4. Writes atomically via a `.tmp` file + `os.replace`
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

hooks = settings.get("hooks")
if not isinstance(hooks, dict):
    hooks = {}
    settings["hooks"] = hooks

def ensure_entry(event, matcher, script_name, timeout_ms):
    arr = hooks.get(event)
    if not isinstance(arr, list):
        arr = []
        hooks[event] = arr
    for m in arr:
        try:
            for h in m.get("hooks", []):
                if "squeez" in str(h.get("command", "")):
                    return
        except Exception:
            continue
    arr.append({
        "matcher": matcher,
        "hooks": [{
            "name": "squeez-" + event.lower(),
            "type": "command",
            "command": os.path.join(hooks_dir, script_name),
            "timeout": timeout_ms,
        }],
    })

ensure_entry("SessionStart", ".*", "gemini-session-start.sh", 5000)
ensure_entry("BeforeTool",    ".*", "gemini-before-tool.sh", 5000)
ensure_entry("AfterTool",     ".*", "gemini-after-tool.sh",  3000)

tmp = path + ".tmp"
os.makedirs(os.path.dirname(path), exist_ok=True)
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

hooks = settings.get("hooks")
if isinstance(hooks, dict):
    for event in ("SessionStart", "BeforeTool", "AfterTool"):
        arr = hooks.get(event)
        if isinstance(arr, list):
            hooks[event] = [
                m for m in arr
                if not any("squeez" in str(h.get("command", "")) for h in m.get("hooks", []))
            ]
            if not hooks[event]:
                del hooks[event]
    if not hooks:
        settings.pop("hooks", None)

tmp = path + ".tmp"
with open(tmp, "w") as f:
    json.dump(settings, f, indent=2)
os.replace(tmp, path)
"#;

pub struct GeminiCliAdapter;

impl GeminiCliAdapter {
    fn gemini_dir() -> PathBuf {
        PathBuf::from(format!("{}/.gemini", home_dir()))
    }
    fn hooks_dir() -> PathBuf {
        Self::gemini_dir().join("squeez").join("hooks")
    }
    fn settings_path() -> PathBuf {
        Self::gemini_dir().join("settings.json")
    }
    fn memory_path() -> PathBuf {
        Self::gemini_dir().join("GEMINI.md")
    }

    fn write_hook_scripts(hooks_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(hooks_dir)?;
        for (name, body) in [
            ("gemini-session-start.sh", SESSION_START_SCRIPT),
            ("gemini-before-tool.sh", BEFORE_TOOL_SCRIPT),
            ("gemini-after-tool.sh", AFTER_TOOL_SCRIPT),
        ] {
            let path = hooks_dir.join(name);
            std::fs::write(&path, body)?;
            // Make the hook scripts executable (Gemini invokes them as commands).
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(0o755);
                let _ = std::fs::set_permissions(&path, perms);
            }
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
}

impl HostAdapter for GeminiCliAdapter {
    fn name(&self) -> &'static str {
        "gemini"
    }

    fn is_installed(&self) -> bool {
        Self::gemini_dir().exists()
    }

    fn data_dir(&self) -> PathBuf {
        Self::gemini_dir().join("squeez")
    }

    fn capabilities(&self) -> HostCaps {
        HostCaps::BASH_WRAP | HostCaps::SESSION_MEM | HostCaps::BUDGET_SOFT
    }

    fn install(&self, _bin_path: &Path) -> std::io::Result<()> {
        let hooks_dir = Self::hooks_dir();
        Self::write_hook_scripts(&hooks_dir)?;
        let settings = Self::settings_path();
        Self::run_python(
            PATCH_SCRIPT,
            &[
                settings.to_str().unwrap_or(""),
                hooks_dir.to_str().unwrap_or(""),
            ],
        )?;
        Ok(())
    }

    fn uninstall(&self) -> std::io::Result<()> {
        let hooks_dir = Self::hooks_dir();
        if hooks_dir.exists() {
            let _ = std::fs::remove_dir_all(&hooks_dir);
        }
        let settings = Self::settings_path();
        if settings.exists() {
            Self::run_python(UNPATCH_SCRIPT, &[settings.to_str().unwrap_or("")])?;
        }
        let memory = Self::memory_path();
        if memory.exists() {
            let existing = std::fs::read_to_string(&memory).unwrap_or_default();
            let cleaned = strip_squeez_block(&existing);
            let _ = std::fs::write(&memory, cleaned);
        }
        Ok(())
    }

    fn inject_memory(&self, cfg: &Config, summaries: &[Summary]) -> std::io::Result<()> {
        let path = Self::memory_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let existing = std::fs::read_to_string(&path).unwrap_or_default();

        let mut block = String::from("<!-- squeez:start -->\n");
        if let Some(banner) =
            memory_size::size_warning(&existing, "GEMINI.md", cfg.memory_file_warn_tokens)
        {
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
        // Soft-budget hint (Gemini BUDGET_HARD is pending upstream rewrite schema docs).
        block.push_str(&format!(
            "\n## Tool-output budget (soft enforcement)\nWhen using read_file / grep-equivalent tools, cap output to ~{} lines unless the user explicitly asks for more.\n",
            cfg.read_max_lines
        ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_squeez_block_inside_existing_gemini_md() {
        let s = "# project rules\n<!-- squeez:start -->\nfoo\n<!-- squeez:end -->\n## after\n";
        let out = strip_squeez_block(s);
        assert!(!out.contains("<!-- squeez:start -->"));
        assert!(out.contains("# project rules"));
        assert!(out.contains("## after"));
    }
}
