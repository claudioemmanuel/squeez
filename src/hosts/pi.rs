//! Pi coding agent adapter (https://pi.dev)
//!
//! Pi auto-discovers TypeScript extensions from `~/.pi/agent/extensions/`.
//! No JSON patching is needed — squeez writes its extension directly to
//! `~/.pi/agent/extensions/squeez/index.ts` and Pi loads it on next start.
//!
//! Memory injection writes `~/.pi/agent/skills/squeez/SKILL.md`. Pi's skill
//! system includes skill descriptions in every system prompt (progressive
//! disclosure: full instructions load on-demand when the model invokes them).
//!
//! Capabilities: `BASH_WRAP | SESSION_MEM | BUDGET_HARD`.
//! `BUDGET_HARD` is achieved via the `tool_result` event — the extension
//! pipes tool output through `squeez filter <tool>` before it reaches the LLM.

use std::path::{Path, PathBuf};

use crate::commands::persona;
use crate::config::Config;
use crate::memory::Summary;
use crate::session::home_dir;

use super::{memory_size, HostAdapter, HostCaps};

const EXTENSION_SCRIPT: &str = include_str!("../../hooks/pi-extension.ts");

const SKILL_FRONTMATTER: &str =
    "---\nname: squeez\ndescription: Always-on token compressor. Apply terse persona, keep tool output within budget, track cross-session context.\n---\n";

pub struct PiAdapter;

impl PiAdapter {
    fn pi_dir() -> PathBuf {
        PathBuf::from(format!("{}/.pi/agent", home_dir()))
    }
    fn extension_dir() -> PathBuf {
        Self::pi_dir().join("extensions").join("squeez")
    }
    fn skill_dir() -> PathBuf {
        Self::pi_dir().join("skills").join("squeez")
    }
    fn extension_path() -> PathBuf {
        Self::extension_dir().join("index.ts")
    }
    fn skill_md_path() -> PathBuf {
        Self::skill_dir().join("SKILL.md")
    }
}

impl HostAdapter for PiAdapter {
    fn name(&self) -> &'static str {
        "pi"
    }

    fn is_installed(&self) -> bool {
        Self::pi_dir().exists()
    }

    fn data_dir(&self) -> PathBuf {
        Self::pi_dir().join("squeez")
    }

    fn capabilities(&self) -> HostCaps {
        HostCaps::BASH_WRAP | HostCaps::SESSION_MEM | HostCaps::BUDGET_HARD
    }

    fn install(&self, _bin_path: &Path) -> std::io::Result<()> {
        let ext_dir = Self::extension_dir();
        std::fs::create_dir_all(&ext_dir)?;
        std::fs::write(Self::extension_path(), EXTENSION_SCRIPT)?;

        let skill_dir = Self::skill_dir();
        std::fs::create_dir_all(&skill_dir)?;
        let skill_path = Self::skill_md_path();
        if !skill_path.exists() {
            std::fs::write(&skill_path, SKILL_FRONTMATTER)?;
        }
        Ok(())
    }

    fn uninstall(&self) -> std::io::Result<()> {
        let ext_dir = Self::extension_dir();
        if ext_dir.exists() {
            std::fs::remove_dir_all(&ext_dir)?;
        }
        let skill_dir = Self::skill_dir();
        if skill_dir.exists() {
            std::fs::remove_dir_all(&skill_dir)?;
        }
        Ok(())
    }

    fn inject_memory(&self, cfg: &Config, summaries: &[Summary]) -> std::io::Result<()> {
        let path = Self::skill_md_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        let (frontmatter, body) = split_frontmatter(&existing);

        let mut block = String::from("<!-- squeez:start -->\n");
        if let Some(banner) =
            memory_size::size_warning(&existing, "SKILL.md", cfg.memory_file_warn_tokens)
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

        let cleaned_body = strip_squeez_block(body);
        let new_body = format!("{}\n{}", block, cleaned_body.trim_start());
        let fm = if frontmatter.is_empty() {
            SKILL_FRONTMATTER.to_string()
        } else {
            frontmatter.to_string()
        };
        std::fs::write(&path, format!("{}{}", fm, new_body))
    }
}

/// Split a markdown file into `(frontmatter, body)`.
/// `frontmatter` includes both `---` delimiters and their newlines.
/// Returns `("", s)` when no valid frontmatter is found.
fn split_frontmatter(s: &str) -> (&str, &str) {
    if !s.starts_with("---\n") {
        return ("", s);
    }
    if let Some(end_offset) = s[4..].find("\n---\n") {
        let split = 4 + end_offset + 5;
        (&s[..split], &s[split..])
    } else {
        ("", s)
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
    fn strip_squeez_block_inside_existing_skill_md() {
        let s = "# context\n<!-- squeez:start -->\nfoo\n<!-- squeez:end -->\n## after\n";
        let out = strip_squeez_block(s);
        assert!(!out.contains("<!-- squeez:start -->"));
        assert!(out.contains("# context"));
        assert!(out.contains("## after"));
    }

    #[test]
    fn split_frontmatter_extracts_yaml_block() {
        let s = "---\nname: squeez\n---\nbody here\n";
        let (fm, body) = split_frontmatter(s);
        assert!(fm.contains("name: squeez"));
        assert_eq!(body.trim(), "body here");
    }

    #[test]
    fn split_frontmatter_no_frontmatter() {
        let s = "no frontmatter here\n";
        let (fm, body) = split_frontmatter(s);
        assert!(fm.is_empty());
        assert_eq!(body, "no frontmatter here\n");
    }

    #[test]
    fn split_frontmatter_only_opening_delimiter() {
        let s = "---\nincomplete";
        let (fm, body) = split_frontmatter(s);
        assert!(fm.is_empty());
        assert_eq!(body, s);
    }
}
