//! Hermes Agent adapter.
//!
//! [Hermes Agent](https://github.com/NousResearch/hermes-agent) by Nous Research
//! is a general-purpose AI agent with a plugin system at `$HERMES_HOME/plugins/`
//! (`~/.hermes/plugins/` when the variable is unset).
//! A directory plugin is a Python package with a `plugin.yaml` manifest plus an
//! `__init__.py` exposing `register(ctx)`; both files are required for Hermes to
//! discover it. Discovered plugins are opt-in and must be enabled once via
//! `hermes plugins enable squeez-fallback`.
//!
//! Hermes fires `pre_tool_call` and `post_tool_call` hooks for every tool
//! invocation. The squeez plugin wraps terminal commands via `pre_tool_call`,
//! similar to Claude Code's PreToolUse hook.
//!
//! Capability ceiling: `BASH_WRAP | SESSION_MEM`.
//! Hermes does not expose programmatic Read/Grep budget injection — the
//! `BUDGET_HARD` flag is not set. Budget hints could be injected into the
//! system prompt via `inject_memory`, but Hermes users typically configure
//! their SOUL.md / config.yaml system_prompt manually.
//!
//! Memory injection: Hermes loads its system prompt from config.yaml at
//! session start and does not auto-load a markdown file the way Claude Code
//! loads CLAUDE.md. The adapter writes a squeez persona block into
//! `~/.hermes/profiles/default/SOUL.md` if it exists, which is the canonical
//! "personality" file for the default profile.

use std::path::{Path, PathBuf};

use crate::commands::focus;
use crate::commands::persona;
use crate::config::Config;
use crate::memory::Summary;
use crate::session::home_dir;

use super::{memory_size, HostAdapter, HostCaps};

/// The Python plugin dropped into ~/.hermes/plugins/squeez-fallback/__init__.py
const HERMES_PLUGIN: &str = include_str!("../../plugins/hermes/__init__.py");

/// The manifest dropped alongside it. Hermes only discovers a directory plugin
/// when both `plugin.yaml` and `__init__.py` are present.
const HERMES_MANIFEST: &str = include_str!("../../plugins/hermes/plugin.yaml");

/// Split out from [`HermesAdapter::hermes_dir`] so the override is testable
/// without mutating the process environment.
fn hermes_dir_from(hermes_home: Option<String>, home: &str) -> PathBuf {
    match hermes_home.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(format!("{}/.hermes", home)),
    }
}

pub struct HermesAdapter;

impl HermesAdapter {
    /// Where Hermes keeps its plugins and profiles.
    ///
    /// `HERMES_HOME` wins when set — a Windows install at
    /// `%LOCALAPPDATA%\hermes` lives nowhere near `~/.hermes`, so probing only
    /// the default made `is_installed()` false and `squeez setup --host=hermes`
    /// silently skip a perfectly good Hermes (issue #208).
    fn hermes_dir() -> PathBuf {
        hermes_dir_from(std::env::var("HERMES_HOME").ok(), &home_dir())
    }

    fn plugins_dir() -> PathBuf {
        Self::hermes_dir().join("plugins")
    }

    fn squeez_plugin_dir() -> PathBuf {
        Self::plugins_dir().join("squeez-fallback")
    }

    fn soul_md_path() -> PathBuf {
        Self::hermes_dir()
            .join("profiles")
            .join("default")
            .join("SOUL.md")
    }
}

impl HostAdapter for HermesAdapter {
    fn name(&self) -> &'static str {
        "hermes"
    }

    fn is_installed(&self) -> bool {
        // Detect Hermes by checking the install root (HERMES_HOME, else
        // ~/.hermes) for a hermes-agent subdir or a config.yaml — either
        // indicates a real installation.
        let dir = Self::hermes_dir();
        if !dir.exists() {
            return false;
        }
        dir.join("hermes-agent").exists() || dir.join("config.yaml").exists()
    }

    fn data_dir(&self) -> PathBuf {
        std::env::var("SQUEEZ_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| Self::hermes_dir().join("squeez"))
    }

    fn capabilities(&self) -> HostCaps {
        // Hermes has pre_tool_call (BASH_WRAP) and session start (SESSION_MEM).
        // No programmatic Read/Grep budget injection (BUDGET_HARD) or soft
        // budget hints via markdown (BUDGET_SOFT).
        HostCaps::BASH_WRAP | HostCaps::SESSION_MEM
    }

    fn install(&self, _bin_path: &Path) -> std::io::Result<()> {
        let plugin_dir = Self::squeez_plugin_dir();
        std::fs::create_dir_all(&plugin_dir)?;

        // Write the Python plugin + its manifest. Directory plugins require
        // both files; without plugin.yaml Hermes will not discover the plugin.
        std::fs::write(plugin_dir.join("__init__.py"), HERMES_PLUGIN)?;
        std::fs::write(plugin_dir.join("plugin.yaml"), HERMES_MANIFEST)?;

        // Create data directories
        let data = self.data_dir();
        std::fs::create_dir_all(data.join("sessions"))?;
        std::fs::create_dir_all(data.join("memory"))?;

        // Hermes discovers the plugin from ~/.hermes/plugins/ on next session
        // start, but directory plugins are opt-in — the user must enable it once:
        //     hermes plugins enable squeez-fallback
        eprintln!(
            "squeez: Hermes plugin installed. Enable it once with:\n    hermes plugins enable squeez-fallback"
        );
        Ok(())
    }

    fn uninstall(&self) -> std::io::Result<()> {
        let plugin_dir = Self::squeez_plugin_dir();
        if plugin_dir.exists() {
            let _ = std::fs::remove_dir_all(&plugin_dir);
        }

        // Clean persona block from SOUL.md if present
        let soul = Self::soul_md_path();
        if soul.exists() {
            let existing = std::fs::read_to_string(&soul).unwrap_or_default();
            let cleaned = strip_squeez_block(&existing);
            if cleaned != existing {
                let _ = std::fs::write(&soul, cleaned);
            }
        }
        Ok(())
    }

    fn inject_memory(&self, cfg: &Config, summaries: &[Summary]) -> std::io::Result<()> {
        let soul = Self::soul_md_path();
        if !soul.exists() {
            // SOUL.md doesn't exist — skip memory injection. Hermes users
            // configure their system prompt via config.yaml; we don't create
            // SOUL.md to avoid overriding their setup.
            return Ok(());
        }

        let existing = std::fs::read_to_string(&soul).unwrap_or_default();

        let mut block = String::from("<!-- squeez:start -->\n");
        if let Some(banner) =
            memory_size::size_warning(&existing, "SOUL.md", cfg.memory_file_warn_tokens)
        {
            block.push_str(&banner);
        }
        block.push_str("## squeez — always-on compression\n\n");
        block.push_str(&format!(
            "Persona: {}{} | Bash compression: ON | Memory: ON\n\n",
            persona::as_str(cfg.persona),
            focus::banner_suffix(cfg.focus)
        ));
        let persona_text = persona::text_with_lang(cfg.persona, &cfg.lang);
        if !persona_text.is_empty() {
            block.push_str(persona_text);
            if !persona_text.ends_with('\n') {
                block.push('\n');
            }
        }
        let focus_text = focus::text_with_lang(cfg.focus, &cfg.lang);
        if !focus_text.is_empty() {
            block.push('\n');
            block.push_str(focus_text);
            if !focus_text.ends_with('\n') {
                block.push('\n');
            }
        }
        for s in summaries {
            block.push_str(&format!("- {}\n", s.display_line()));
        }
        block.push_str("<!-- squeez:end -->\n");

        let cleaned = strip_squeez_block(&existing);
        let contents = format!("{}\n{}", block, cleaned.trim_start());
        std::fs::write(&soul, contents)
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

    /// #208: a Windows Hermes at %LOCALAPPDATA%\hermes was never detected,
    /// because only ~/.hermes was probed — so `squeez setup --host=hermes`
    /// silently skipped a perfectly good install.
    #[test]
    fn hermes_home_overrides_the_default_location() {
        assert_eq!(
            hermes_dir_from(Some("C:/Users/J/AppData/Local/hermes".into()), "/home/j"),
            PathBuf::from("C:/Users/J/AppData/Local/hermes")
        );
    }

    #[test]
    fn default_location_is_used_when_hermes_home_is_unset_or_blank() {
        assert_eq!(hermes_dir_from(None, "/home/j"), PathBuf::from("/home/j/.hermes"));
        assert_eq!(hermes_dir_from(Some("  ".into()), "/home/j"), PathBuf::from("/home/j/.hermes"));
    }

    #[test]
    fn strip_squeez_block_inside_existing_soul_md() {
        let s = "# SOUL\n<!-- squeez:start -->\nfoo\n<!-- squeez:end -->\n## after\n";
        let out = strip_squeez_block(s);
        assert!(!out.contains("<!-- squeez:start -->"));
        assert!(out.contains("# SOUL"));
        assert!(out.contains("## after"));
    }

    #[test]
    fn strip_squeez_block_passthrough_when_absent() {
        let s = "# SOUL\nnothing to strip\n";
        assert_eq!(strip_squeez_block(s), s);
    }

    #[test]
    fn name_is_hermes() {
        let a = HermesAdapter;
        assert_eq!(a.name(), "hermes");
    }

    #[test]
    fn manifest_declares_required_fields() {
        // Hermes won't discover a directory plugin without these.
        assert!(HERMES_MANIFEST.contains("name: squeez-fallback"));
        assert!(HERMES_MANIFEST.contains("version:"));
        assert!(HERMES_MANIFEST.contains("description:"));
        assert!(HERMES_MANIFEST.contains("pre_tool_call"));
    }
}
