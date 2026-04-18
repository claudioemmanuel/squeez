//! Copilot CLI adapter: writes the squeez memory block into
//! `~/.copilot/copilot-instructions.md`.
//!
//! Behaviour migrated byte-for-byte from the pre-adapter
//! `src/commands/init.rs::inject_copilot_instructions()`.

use std::path::{Path, PathBuf};

use crate::commands::persona;
use crate::config::Config;
use crate::memory::Summary;
use crate::session::home_dir;

use super::{HostAdapter, HostCaps};

pub struct CopilotCliAdapter;

impl HostAdapter for CopilotCliAdapter {
    fn name(&self) -> &'static str {
        "copilot"
    }

    fn is_installed(&self) -> bool {
        Path::new(&format!("{}/.copilot", home_dir())).exists()
    }

    fn data_dir(&self) -> PathBuf {
        // Honour SQUEEZ_DIR override (set by run_copilot for tests), else
        // default to ~/.copilot/squeez — matches the pre-refactor path.
        std::env::var("SQUEEZ_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(format!("{}/.copilot/squeez", home_dir())))
    }

    fn capabilities(&self) -> HostCaps {
        HostCaps::BASH_WRAP | HostCaps::SESSION_MEM | HostCaps::BUDGET_HARD
    }

    fn install(&self, _bin_path: &Path) -> std::io::Result<()> {
        // TODO US-007: write ~/.copilot/settings.json hook entries here.
        Ok(())
    }

    fn uninstall(&self) -> std::io::Result<()> {
        // TODO US-007: remove squeez entries from ~/.copilot/settings.json.
        Ok(())
    }

    /// Replaces the squeez block (`<!-- squeez:start --> … <!-- squeez:end -->`)
    /// in `~/.copilot/copilot-instructions.md`, creating the file if absent.
    fn inject_memory(&self, cfg: &Config, summaries: &[Summary]) -> std::io::Result<()> {
        let home = home_dir();
        let path = format!("{}/.copilot/copilot-instructions.md", home);
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
        let persona_text = persona::text_with_lang(cfg.persona, &cfg.lang);
        if !persona_text.is_empty() {
            block.push('\n');
            block.push_str(persona_text);
        }
        block.push_str("<!-- squeez:end -->\n");

        // Strip previous squeez block if present
        let cleaned = if existing.contains("<!-- squeez:start -->") {
            let start = existing.find("<!-- squeez:start -->").unwrap_or(0);
            let end = existing
                .find("<!-- squeez:end -->")
                .map(|i| i + "<!-- squeez:end -->".len() + 1) // include newline
                .unwrap_or(start);
            format!("{}{}", &existing[..start], &existing[end.min(existing.len())..])
        } else {
            existing
        };

        // Prepend the fresh block
        let contents = format!("{}\n{}", block, cleaned.trim_start());
        std::fs::write(&path, contents)
    }
}
