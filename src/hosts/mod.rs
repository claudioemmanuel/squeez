//! Host adapter registry — one implementation per supported CLI agent.
//!
//! Each CLI (Claude Code, Copilot CLI, OpenCode, Gemini CLI, Codex CLI) has
//! a distinct config file layout, hook format, and memory-injection channel.
//! The `HostAdapter` trait abstracts those differences so `squeez setup`,
//! `squeez uninstall`, and the session-start flow can treat every host
//! uniformly.
//!
//! Real adapters live in dedicated per-host modules (`claude_code`,
//! `copilot`, ...). Hosts still waiting on migration (US-004 .. US-006)
//! keep their stub impl at the bottom of this file.

pub mod buddy;
pub mod claude_code;
pub mod codex;
pub mod copilot;
pub mod gemini;
pub mod hermes;
pub mod memory_size;
pub mod opencode;
pub mod pi;
pub mod settings_json;
pub use claude_code::ClaudeCodeAdapter;
pub use codex::CodexCliAdapter;
pub use copilot::CopilotCliAdapter;
pub use gemini::GeminiCliAdapter;
pub use hermes::HermesAdapter;
pub use opencode::OpenCodeAdapter;
pub use pi::PiAdapter;

use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::memory::Summary;

// ── Capability bitflags (zero-dep: plain u8 newtype) ───────────────────────

/// What a host can do natively. See the host capability matrix in
/// `docs/superpowers/specs/2026-04-18-cli-host-coverage-design.md`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct HostCaps(pub u8);

impl HostCaps {
    pub const NONE: Self = Self(0);
    /// Host can intercept Bash tool calls before execution.
    pub const BASH_WRAP: Self = Self(0b0001);
    /// Host offers a session-start hook OR a stable memory file we can inject into.
    pub const SESSION_MEM: Self = Self(0b0010);
    /// Host can rewrite tool_input for Read/Grep/Glob (programmatic enforcement).
    pub const BUDGET_HARD: Self = Self(0b0100);
    /// Host only supports soft budget hints via an auto-loaded markdown file.
    pub const BUDGET_SOFT: Self = Self(0b1000);

    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

impl std::ops::BitOr for HostCaps {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

// ── Trait ──────────────────────────────────────────────────────────────────

pub trait HostAdapter {
    /// Stable slug used on the command line (e.g. `squeez setup --host=gemini`).
    fn name(&self) -> &'static str;

    /// Returns true when the host's config directory is present on disk,
    /// indicating the user has the CLI installed and squeez should offer to
    /// integrate with it.
    fn is_installed(&self) -> bool;

    /// Squeez's per-host state directory (sessions/, memory/, config.ini).
    fn data_dir(&self) -> PathBuf;

    /// What this host supports natively.
    fn capabilities(&self) -> HostCaps;

    /// Register squeez hooks/plugin into the host's settings. Idempotent.
    fn install(&self, bin_path: &Path) -> std::io::Result<()>;

    /// Remove squeez entries from the host's settings, leaving everything
    /// else intact. Idempotent.
    fn uninstall(&self) -> std::io::Result<()>;

    /// Write the squeez memory block into the host's auto-loaded instructions
    /// file (CLAUDE.md / copilot-instructions.md / GEMINI.md / AGENTS.md).
    fn inject_memory(&self, cfg: &Config, summaries: &[Summary]) -> std::io::Result<()>;

    /// Whether [`Self::inject_memory`] puts the persona/focus text into a file
    /// this host auto-loads at every session start.
    ///
    /// When true, `squeez init` leaves that text out of its session banner:
    /// otherwise both channels reach the model and the persona is taught twice
    /// per session — ~145 tokens paid twice, in a tool whose whole point is not
    /// paying them (#218). The instructions file is the copy worth keeping: it
    /// is language-aware and the host re-loads it after a compaction, while the
    /// banner carries the genuinely per-session lines the file cannot.
    ///
    /// The default holds for every adapter that unconditionally writes an
    /// instructions file; a host that may skip injection overrides it.
    fn injects_persona(&self, cfg: &Config) -> bool {
        !crate::commands::persona::text_with_lang(cfg.persona, &cfg.lang).is_empty()
            || !crate::commands::focus::text_with_lang(cfg.focus, &cfg.lang).is_empty()
    }
}

// ── Registry ───────────────────────────────────────────────────────────────

pub fn all_hosts() -> Vec<Box<dyn HostAdapter>> {
    vec![
        Box::new(ClaudeCodeAdapter),
        Box::new(CopilotCliAdapter),
        Box::new(OpenCodeAdapter),
        Box::new(GeminiCliAdapter),
        Box::new(CodexCliAdapter),
        Box::new(PiAdapter),
        Box::new(HermesAdapter),
    ]
}

/// Look up an adapter by its `name()` slug.
pub fn find(slug: &str) -> Option<Box<dyn HostAdapter>> {
    all_hosts().into_iter().find(|h| h.name() == slug)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_wrap_flag_round_trips() {
        let c = HostCaps::BASH_WRAP | HostCaps::SESSION_MEM;
        assert!(c.contains(HostCaps::BASH_WRAP));
        assert!(c.contains(HostCaps::SESSION_MEM));
        assert!(!c.contains(HostCaps::BUDGET_HARD));
    }

    #[test]
    fn find_returns_claude_code() {
        let a = find("claude-code").expect("adapter");
        assert_eq!(a.name(), "claude-code");
    }

    #[test]
    fn find_returns_none_for_unknown() {
        assert!(find("nonexistent").is_none());
    }

    #[test]
    fn find_returns_hermes() {
        let a = find("hermes").expect("adapter");
        assert_eq!(a.name(), "hermes");
    }

    #[test]
    fn hosts_with_an_instructions_file_own_the_persona_channel() {
        // #218: every adapter but Hermes writes an auto-loaded instructions
        // file unconditionally, so `squeez init` must not repeat the text.
        let mut cfg = Config::default();
        cfg.persona = crate::commands::persona::Persona::Ultra;
        for h in all_hosts() {
            if h.name() == "hermes" {
                continue; // conditional on SOUL.md existing on this machine
            }
            assert!(
                h.injects_persona(&cfg),
                "{} should own the persona channel",
                h.name()
            );
        }
    }

    #[test]
    fn nothing_to_inject_leaves_the_channel_free() {
        let mut cfg = Config::default();
        cfg.persona = crate::commands::persona::Persona::Off;
        cfg.focus = crate::commands::focus::Focus::Off;
        let a = find("claude-code").expect("adapter");
        assert!(!a.injects_persona(&cfg));
    }
}
