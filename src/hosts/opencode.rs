//! OpenCode adapter: drops squeez's ESM plugin into `~/.config/opencode/plugins/`
//! and seeds `~/.config/opencode/AGENTS.md` with the squeez memory block.
//!
//! Research on OpenCode hook surface (2026-04-18):
//!   - `tool.execute.before` can mutate `output.args` for ANY tool (bash,
//!     read, grep, glob) — enough for BASH_WRAP + BUDGET_HARD.
//!   - `session.created` fires once per session — the plugin shells out to
//!     `squeez init --host=opencode` to finalize the previous session and
//!     refresh AGENTS.md before OpenCode builds its first prompt.
//!   - OpenCode auto-loads `~/.config/opencode/AGENTS.md` at session start,
//!     which is the channel we use for SESSION_MEM.
//!
//! See docs/superpowers/specs/2026-04-18-cli-host-coverage-design.md.

use std::path::{Path, PathBuf};

use crate::commands::persona;
use crate::config::Config;
use crate::memory::Summary;
use crate::session::home_dir;

use super::{memory_size, HostAdapter, HostCaps};

const PLUGIN_FILENAME: &str = "squeez.js";

/// The ESM plugin bundled by squeez (source of truth lives in
/// `opencode-plugin/squeez.js`). When `install()` runs it copies the file
/// from the on-disk source location or, if that's not accessible (e.g. the
/// binary is distributed via npm/curl without the repo), falls back to this
/// embedded copy.
const PLUGIN_SOURCE: &str = include_str!("../../opencode-plugin/squeez.js");

pub struct OpenCodeAdapter;

impl OpenCodeAdapter {
    fn opencode_config_dir() -> PathBuf {
        let xdg = std::env::var("XDG_CONFIG_HOME")
            .unwrap_or_else(|_| format!("{}/.config", home_dir()));
        PathBuf::from(xdg).join("opencode")
    }

    fn plugin_path() -> PathBuf {
        Self::opencode_config_dir().join("plugins").join(PLUGIN_FILENAME)
    }

    fn agents_md_path() -> PathBuf {
        Self::opencode_config_dir().join("AGENTS.md")
    }
}

impl HostAdapter for OpenCodeAdapter {
    fn name(&self) -> &'static str {
        "opencode"
    }

    fn is_installed(&self) -> bool {
        Self::opencode_config_dir().exists()
    }

    fn data_dir(&self) -> PathBuf {
        Self::opencode_config_dir().join("squeez")
    }

    fn capabilities(&self) -> HostCaps {
        HostCaps::BASH_WRAP | HostCaps::SESSION_MEM | HostCaps::BUDGET_HARD
    }

    fn install(&self, _bin_path: &Path) -> std::io::Result<()> {
        let plugins_dir = Self::opencode_config_dir().join("plugins");
        std::fs::create_dir_all(&plugins_dir)?;
        std::fs::write(Self::plugin_path(), PLUGIN_SOURCE)?;
        Ok(())
    }

    fn uninstall(&self) -> std::io::Result<()> {
        let plugin = Self::plugin_path();
        if plugin.exists() {
            std::fs::remove_file(&plugin)?;
        }
        // Strip the squeez block from AGENTS.md (leave the rest of the file intact).
        let agents = Self::agents_md_path();
        if agents.exists() {
            let existing = std::fs::read_to_string(&agents).unwrap_or_default();
            let cleaned = strip_squeez_block(&existing);
            let _ = std::fs::write(&agents, cleaned);
        }
        Ok(())
    }

    /// Writes a `<!-- squeez:start --> … <!-- squeez:end -->` block into
    /// `~/.config/opencode/AGENTS.md`, which OpenCode auto-loads at every
    /// session start (confirmed by OpenCode docs on rules precedence).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_squeez_block_removes_inline_block() {
        let s = "# other rules\n<!-- squeez:start -->\nfoo\n<!-- squeez:end -->\n## remainder\n";
        let out = strip_squeez_block(s);
        assert!(!out.contains("<!-- squeez:start -->"));
        assert!(out.contains("# other rules"));
        assert!(out.contains("## remainder"));
    }

    #[test]
    fn strip_squeez_block_preserves_file_without_block() {
        let s = "# just regular content\n";
        assert_eq!(strip_squeez_block(s), s);
    }
}
