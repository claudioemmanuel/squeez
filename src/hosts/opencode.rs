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

use crate::commands::focus;
use crate::commands::persona;
use crate::config::Config;
use crate::memory::Summary;
use crate::session::home_dir;

use super::{memory_file, memory_size, HostAdapter, HostCaps};

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

    /// Where squeez kept OpenCode's state before #270: inside the config tree
    /// OpenCode watches, so every session/memory write was a config event.
    fn legacy_data_dir() -> PathBuf {
        Self::opencode_config_dir().join("squeez")
    }
}

/// `$XDG_STATE_HOME/squeez/opencode`, default `~/.local/state/squeez/opencode`
/// on every platform. Outside OpenCode's watched config tree (#270), and
/// derived the same way by `opencode-plugin/squeez.js` (`SQUEEZ_DIR`), which
/// must stay in step with this.
pub fn state_dir() -> PathBuf {
    let base = std::env::var("XDG_STATE_HOME")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| format!("{}/.local/state", home_dir()));
    PathBuf::from(base).join("squeez").join("opencode")
}

/// Move `legacy` to `target` when only the legacy store exists. When both do,
/// carry over just a `config.ini` the new store lacks: a missing config is not
/// neutral (the defaults turn `auto_compress_md` on). Best effort — a failed
/// move leaves the legacy store where it was.
fn migrate(legacy: &Path, target: &Path) {
    if !legacy.is_dir() {
        return;
    }
    if !target.exists() {
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::rename(legacy, target).is_ok() {
            return;
        }
        // Across filesystems a rename fails: copy, then drop the old store
        // only if the copy is whole.
        if copy_dir(legacy, target).is_ok() {
            let _ = std::fs::remove_dir_all(legacy);
        }
        return;
    }
    let config = target.join("config.ini");
    if !config.exists() {
        let _ = std::fs::copy(legacy.join("config.ini"), config);
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

impl HostAdapter for OpenCodeAdapter {
    fn name(&self) -> &'static str {
        "opencode"
    }

    fn is_installed(&self) -> bool {
        Self::opencode_config_dir().exists()
    }

    fn data_dir(&self) -> PathBuf {
        state_dir()
    }

    fn migrate_data_dir(&self) {
        migrate(&Self::legacy_data_dir(), &state_dir());
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
            if let Ok(file) = memory_file::open(&agents) {
                let cleaned = strip_squeez_block(file.existing());
                let _ = file.commit(&cleaned);
            }
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
        let file = memory_file::open(&path)?;
        let existing = file.existing().to_string();

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
        // Unchanged content is not rewritten; see `MemoryFile::commit`.
        file.commit(&contents)
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

    fn tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "squeez-oc-migrate-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn migrate_moves_the_whole_legacy_store() {
        let root = tmp("move");
        let legacy = root.join("config/opencode/squeez");
        std::fs::create_dir_all(legacy.join("memory")).unwrap();
        std::fs::write(legacy.join("config.ini"), "auto_compress_md = false\n").unwrap();
        std::fs::write(legacy.join("memory/summaries.jsonl"), "{}\n").unwrap();
        let target = root.join("state/squeez/opencode");

        migrate(&legacy, &target);

        assert!(!legacy.exists(), "nothing may keep writing into the watched tree");
        assert_eq!(
            std::fs::read_to_string(target.join("config.ini")).unwrap(),
            "auto_compress_md = false\n"
        );
        assert!(target.join("memory/summaries.jsonl").exists());
    }

    /// The defaults turn `auto_compress_md` on, so a new store that lacks the
    /// user's config must not silently start from them.
    #[test]
    fn migrate_carries_config_into_an_existing_store_that_lacks_one() {
        let root = tmp("config");
        let legacy = root.join("legacy");
        let target = root.join("target");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::create_dir_all(target.join("sessions")).unwrap();
        std::fs::write(legacy.join("config.ini"), "lang = pt-BR\n").unwrap();

        migrate(&legacy, &target);

        assert_eq!(std::fs::read_to_string(target.join("config.ini")).unwrap(), "lang = pt-BR\n");
    }

    #[test]
    fn migrate_never_overwrites_the_new_store_config() {
        let root = tmp("keep");
        let legacy = root.join("legacy");
        let target = root.join("target");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(legacy.join("config.ini"), "old\n").unwrap();
        std::fs::write(target.join("config.ini"), "new\n").unwrap();

        migrate(&legacy, &target);

        assert_eq!(std::fs::read_to_string(target.join("config.ini")).unwrap(), "new\n");
    }

    #[test]
    fn migrate_without_a_legacy_store_creates_nothing() {
        let root = tmp("none");
        let target = root.join("target");
        migrate(&root.join("absent"), &target);
        assert!(!target.exists());
    }

    #[test]
    fn strip_squeez_block_preserves_file_without_block() {
        let s = "# just regular content\n";
        assert_eq!(strip_squeez_block(s), s);
    }
}
