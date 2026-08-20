//! Copilot CLI adapter.

use std::path::{Path, PathBuf};

use crate::commands::focus;
use crate::commands::persona;
use crate::config::Config;
use crate::memory::Summary;
use crate::session::home_dir;

use super::{memory_size, settings_json, HostAdapter, HostCaps};

const PRETOOLUSE_SCRIPT: &str = include_str!("../../hooks/copilot-pretooluse.sh");
const SESSION_START_SCRIPT: &str = include_str!("../../hooks/copilot-session-start.sh");
const POSTTOOLUSE_SCRIPT: &str = include_str!("../../hooks/copilot-posttooluse.sh");

/// Events squeez registers in the Copilot CLI settings file (top-level).
const SQUEEZ_EVENTS: [&str; 3] = ["PreToolUse", "SessionStart", "PostToolUse"];

/// Copilot keeps its event map at the top level and takes plain `bash <path>`
/// commands with no timeout.
fn hook_specs(hooks_dir: &Path) -> Vec<settings_json::HookSpec> {
    // Quoted + forward-slash, same as the Claude Code adapter: Copilot CLI runs
    // these through bash on Windows too, where raw `\` separators are eaten as
    // escapes (#209).
    let cmd = |script: &str| format!("bash {}", settings_json::shell_arg(&hooks_dir.join(script)));
    vec![
        settings_json::HookSpec {
            event: "PreToolUse",
            matcher: Some("Bash"),
            command: cmd("copilot-pretooluse.sh"),
            name: None,
            timeout_ms: None,
        },
        settings_json::HookSpec {
            event: "SessionStart",
            matcher: None,
            command: cmd("copilot-session-start.sh"),
            name: None,
            timeout_ms: None,
        },
        settings_json::HookSpec {
            event: "PostToolUse",
            matcher: None,
            command: cmd("copilot-posttooluse.sh"),
            name: None,
            timeout_ms: None,
        },
    ]
}

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

        settings_json::patch_events(
            &Self::settings_path(),
            settings_json::EventRoot::TopLevel,
            &hook_specs(&hooks),
        )?;
        Ok(())
    }

    fn uninstall(&self) -> std::io::Result<()> {
        let settings = Self::settings_path();
        if settings.exists() {
            settings_json::unpatch_events(
                &settings,
                settings_json::EventRoot::TopLevel,
                &SQUEEZ_EVENTS,
            )?;
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
            "Context budget: ~{}K tokens | Compression: {} | Memory: ON | Persona: {}{}\n",
            budget_k,
            cfg.compression_status_label(),
            persona::as_str(cfg.persona),
            focus::banner_suffix(cfg.focus)
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
        let focus_text = focus::text_with_lang(cfg.focus, &cfg.lang);
        if !focus_text.is_empty() {
            block.push('\n');
            block.push_str(focus_text);
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
