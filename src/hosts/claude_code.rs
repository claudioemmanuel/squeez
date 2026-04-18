//! Claude Code adapter: persona injection into `~/.claude/CLAUDE.md`.
//!
//! Behaviour migrated byte-for-byte from the pre-adapter
//! `src/commands/init.rs::inject_claude_md()` to keep existing integration
//! tests (and live installations) green through the refactor.

use std::path::{Path, PathBuf};

use crate::commands::persona;
use crate::config::Config;
use crate::memory::Summary;
use crate::session::home_dir;

use super::{HostAdapter, HostCaps};

pub struct ClaudeCodeAdapter;

impl HostAdapter for ClaudeCodeAdapter {
    fn name(&self) -> &'static str {
        "claude-code"
    }

    fn is_installed(&self) -> bool {
        Path::new(&format!("{}/.claude", home_dir())).exists()
    }

    fn data_dir(&self) -> PathBuf {
        // Honour SQUEEZ_DIR override (set by tests), else ~/.claude/squeez.
        std::env::var("SQUEEZ_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(format!("{}/.claude/squeez", home_dir())))
    }

    fn capabilities(&self) -> HostCaps {
        HostCaps::BASH_WRAP | HostCaps::SESSION_MEM | HostCaps::BUDGET_HARD
    }

    fn install(&self, _bin_path: &Path) -> std::io::Result<()> {
        // TODO US-007: write ~/.claude/settings.json hook entries here.
        Ok(())
    }

    fn uninstall(&self) -> std::io::Result<()> {
        // TODO US-007: remove squeez entries from ~/.claude/settings.json.
        Ok(())
    }

    /// Writes the squeez persona block into `~/.claude/CLAUDE.md` so Claude
    /// Code picks it up natively at every session start. Idempotent:
    /// replaces any existing `<!-- squeez:start --> … <!-- squeez:end -->`
    /// block on subsequent runs.
    ///
    /// Claude Code's session-memory channel is CLAUDE.md (persona only) plus
    /// the stdout banner (summaries) printed by the caller in
    /// `init::run_with_dirs()` — hence this method intentionally ignores the
    /// `summaries` argument, preserving pre-refactor behaviour.
    fn inject_memory(&self, cfg: &Config, _summaries: &[Summary]) -> std::io::Result<()> {
        let home = home_dir();
        let claude_dir = format!("{}/.claude", home);
        let path = format!("{}/CLAUDE.md", claude_dir);

        // Ensure ~/.claude/ exists (it should, but be safe)
        std::fs::create_dir_all(&claude_dir)?;

        let persona_text = persona::text_with_lang(cfg.persona, &cfg.lang);
        if persona_text.is_empty() {
            return Ok(());
        }

        let mut block = String::from("<!-- squeez:start -->\n");
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

        let existing = std::fs::read_to_string(&path).unwrap_or_default();

        let cleaned = if existing.contains("<!-- squeez:start -->") {
            let start = existing.find("<!-- squeez:start -->").unwrap_or(0);
            let end = existing
                .find("<!-- squeez:end -->")
                .map(|i| i + "<!-- squeez:end -->".len() + 1)
                .unwrap_or(start);
            format!(
                "{}{}",
                &existing[..start],
                &existing[end.min(existing.len())..]
            )
        } else {
            existing
        };

        // Prepend squeez block so it's the first thing Claude Code reads
        let contents = format!("{}\n{}", block, cleaned.trim_start());
        std::fs::write(&path, contents)
    }
}
