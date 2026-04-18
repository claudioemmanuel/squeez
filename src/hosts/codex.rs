//! Codex CLI adapter.
//!
//! OpenAI's `codex` CLI (the open-source agentic coder, not the retired
//! Codex model) exposes a Claude-Code-style hook system via
//! `~/.codex/hooks.json` and auto-loads `~/.codex/AGENTS.md` at session
//! start.
//!
//! Capability ceiling: `BASH_WRAP | SESSION_MEM | BUDGET_SOFT`.
//!
//! `PreToolUse` fires on Bash only (no Read/Grep/apply_patch hook surface),
//! and `updatedInput` is parsed-but-unimplemented in Codex as of
//! 2026-04-18 — tracked in openai/codex discussion #2150. Until upstream
//! expands the surface, Read/Grep enforcement ships as **soft** via a
//! prose hint in the AGENTS.md block written by `inject_memory`.
//!
//! JSON patching of `hooks.json` uses a python3 subprocess, consistent
//! with every other adapter in this crate.

use std::path::{Path, PathBuf};

use crate::commands::persona;
use crate::config::Config;
use crate::memory::Summary;
use crate::session::home_dir;

use super::{HostAdapter, HostCaps};

const SESSION_START_SCRIPT: &str = include_str!("../../hooks/codex-session-start.sh");
const PRE_TOOL_USE_SCRIPT: &str = include_str!("../../hooks/codex-pretooluse.sh");
const POST_TOOL_USE_SCRIPT: &str = include_str!("../../hooks/codex-posttooluse.sh");

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
            "type": "command",
            "command": os.path.join(hooks_dir, script_name),
            "timeout": timeout_ms,
        }],
    })

ensure_entry("SessionStart",  ".*",                 "codex-session-start.sh", 5000)
ensure_entry("PreToolUse",    ".*",                 "codex-pretooluse.sh",    5000)
ensure_entry("PostToolUse",   ".*",                 "codex-posttooluse.sh",   3000)

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
    for event in ("SessionStart", "PreToolUse", "PostToolUse"):
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

pub struct CodexCliAdapter;

impl CodexCliAdapter {
    fn codex_dir() -> PathBuf {
        PathBuf::from(format!("{}/.codex", home_dir()))
    }
    fn hooks_dir() -> PathBuf {
        Self::codex_dir().join("squeez").join("hooks")
    }
    fn hooks_json_path() -> PathBuf {
        Self::codex_dir().join("hooks.json")
    }
    fn agents_md_path() -> PathBuf {
        Self::codex_dir().join("AGENTS.md")
    }

    fn write_hook_scripts(hooks_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(hooks_dir)?;
        for (name, body) in [
            ("codex-session-start.sh", SESSION_START_SCRIPT),
            ("codex-pretooluse.sh", PRE_TOOL_USE_SCRIPT),
            ("codex-posttooluse.sh", POST_TOOL_USE_SCRIPT),
        ] {
            let path = hooks_dir.join(name);
            std::fs::write(&path, body)?;
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
                format!("python3 hooks.json patch exited {status}"),
            ));
        }
        Ok(())
    }
}

impl HostAdapter for CodexCliAdapter {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn is_installed(&self) -> bool {
        Self::codex_dir().exists()
    }

    fn data_dir(&self) -> PathBuf {
        Self::codex_dir().join("squeez")
    }

    fn capabilities(&self) -> HostCaps {
        HostCaps::BASH_WRAP | HostCaps::SESSION_MEM | HostCaps::BUDGET_SOFT
    }

    fn install(&self, _bin_path: &Path) -> std::io::Result<()> {
        let hooks_dir = Self::hooks_dir();
        Self::write_hook_scripts(&hooks_dir)?;
        let hooks_json = Self::hooks_json_path();
        Self::run_python(
            PATCH_SCRIPT,
            &[
                hooks_json.to_str().unwrap_or(""),
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
        let hooks_json = Self::hooks_json_path();
        if hooks_json.exists() {
            Self::run_python(UNPATCH_SCRIPT, &[hooks_json.to_str().unwrap_or("")])?;
        }
        let agents = Self::agents_md_path();
        if agents.exists() {
            let existing = std::fs::read_to_string(&agents).unwrap_or_default();
            let cleaned = strip_squeez_block(&existing);
            let _ = std::fs::write(&agents, cleaned);
        }
        Ok(())
    }

    fn inject_memory(&self, cfg: &Config, summaries: &[Summary]) -> std::io::Result<()> {
        let path = Self::agents_md_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let existing = std::fs::read_to_string(&path).unwrap_or_default();

        let mut block = String::from("<!-- squeez:start -->\n");
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
        // Soft budget hint — Codex PreToolUse fires on Bash only (upstream
        // openai/codex#2150), so we nudge the model via AGENTS.md prose.
        block.push_str(&format!(
            "\n## Tool-output budget (soft enforcement)\nWhen using read_file / grep, cap output to ~{} lines unless the user explicitly asks for more.\nWhen using apply_patch on large files, target minimal diffs instead of rewriting whole files.\n",
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
    fn strip_squeez_block_inside_existing_agents_md() {
        let s = "# repo rules\n<!-- squeez:start -->\nfoo\n<!-- squeez:end -->\n## after\n";
        let out = strip_squeez_block(s);
        assert!(!out.contains("<!-- squeez:start -->"));
        assert!(out.contains("# repo rules"));
        assert!(out.contains("## after"));
    }
}
