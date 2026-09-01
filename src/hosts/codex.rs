//! Codex CLI adapter.
//!
//! OpenAI's `codex` CLI (the open-source agentic coder, not the retired
//! Codex model) exposes a Claude-Code-style hook system via
//! `~/.codex/hooks.json` and auto-loads `~/.codex/AGENTS.md` at session
//! start.
//!
//! Capability ceiling: `BASH_WRAP | SESSION_MEM | BUDGET_SOFT`.
//!
//! Hook surface as of Codex 0.144.0 (2026-07-09), per source inspection of
//! `codex-rs/core/src/tools/registry.rs` and `codex-rs/hooks/src/engine/output_parser.rs`:
//! - `apply_patch` emits `PreToolUse`/`PostToolUse` (PR openai/codex#18391, 0.123.0)
//! - `updatedInput` rewrites for `PreToolUse` now generalized beyond Bash/shell:
//!   Bash-like tools, `apply_patch`, and MCP tools (PR openai/codex#20527), plus a
//!   default hook contract (`pre_tool_use_payload`/`post_tool_use_payload`/
//!   `with_updated_hook_input`) applied to every ordinary local function tool via
//!   `CoreToolRuntime` (PR openai/codex#23757) — exceptions are hosted tools and
//!   code-mode `wait`/`write_stdin`.
//! - No dedicated `read_file`/`grep` tool handler exists in current codex-rs;
//!   file reads appear to route through the shell/exec tool or MCP, both of
//!   which are covered by the above. Still unconfirmed at runtime (local Codex
//!   install here is broken — `ENOENT` on the vendored binary) — do not flip to
//!   `BUDGET_HARD` until someone confirms live. Tracked in openai/codex#18491.
//!
//! Until confirmed, Read/Grep budget enforcement ships as **soft** via a prose
//! hint in the AGENTS.md block written by `inject_memory`.
//!
//! JSON patching of `hooks.json` uses a python3 subprocess, consistent
//! with every other adapter in this crate.

use std::path::{Path, PathBuf};

use crate::commands::focus;
use crate::commands::persona;
use crate::config::Config;
use crate::memory::Summary;
use crate::session::home_dir;

use super::{memory_size, settings_json, HostAdapter, HostCaps};

const SESSION_START_SCRIPT: &str = include_str!("../../hooks/codex-session-start.sh");
const PRE_TOOL_USE_SCRIPT: &str = include_str!("../../hooks/codex-pretooluse.sh");
const POST_TOOL_USE_SCRIPT: &str = include_str!("../../hooks/codex-posttooluse.sh");

/// Events squeez registers under `hooks` in the Codex CLI hooks.json.
const SQUEEZ_EVENTS: [&str; 3] = ["SessionStart", "PreToolUse", "PostToolUse"];

/// Codex nests its event map under `hooks` and takes a per-hook timeout but no
/// entry name.
///
/// Command must be `bash <path>`, not a bare script path: Codex spawns the
/// command directly rather than handing it to a shell, so on Windows a bare
/// `.sh` path has no interpreter to run it and the hook dies immediately with
/// exit code 1 (every squeez Codex hook, starting with SessionStart).
fn hook_specs(hooks_dir: &Path) -> Vec<settings_json::HookSpec> {
    let spec = |event: &'static str, script: &'static str, timeout_ms: u64| settings_json::HookSpec {
        event,
        matcher: Some(".*"),
        command: format!("bash {}", settings_json::shell_arg(&hooks_dir.join(script))),
        name: None,
        timeout_ms: Some(timeout_ms),
        script,
    };
    vec![
        spec("SessionStart", "codex-session-start.sh", 5000),
        spec("PreToolUse", "codex-pretooluse.sh", 5000),
        spec("PostToolUse", "codex-posttooluse.sh", 3000),
    ]
}

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
        settings_json::patch_events(
            &hooks_json,
            settings_json::EventRoot::Nested,
            &hook_specs(&hooks_dir),
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
            settings_json::unpatch_events(
                &hooks_json,
                settings_json::EventRoot::Nested,
                &SQUEEZ_EVENTS,
            )?;
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
        if let Some(banner) =
            memory_size::size_warning(&existing, "AGENTS.md", cfg.memory_file_warn_tokens)
        {
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
        // Soft budget hint — updatedInput is explicitly unsupported in Codex
        // and read_file/grep have no hook surface (openai/codex#18491), so we
        // nudge the model via AGENTS.md prose.
        block.push_str(&format!(
            "\n## Tool-output budget (soft enforcement)\nWhen using read_file / grep, cap output to ~{} lines unless the user explicitly asks for more.\nWhen using apply_patch on large files, target minimal diffs instead of rewriting whole files.\n",
            cfg.read_max_lines
        ));
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
