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

use super::{memory_file, memory_size, settings_json, HostAdapter, HostCaps};

const SESSION_START_SCRIPT: &str = include_str!("../../hooks/codex-session-start.sh");
const PRE_TOOL_USE_SCRIPT: &str = include_str!("../../hooks/codex-pretooluse.sh");
const POST_TOOL_USE_SCRIPT: &str = include_str!("../../hooks/codex-posttooluse.sh");
const HOOK_SCRIPTS: [(&str, &str); 3] = [
    ("codex-session-start.sh", SESSION_START_SCRIPT),
    ("codex-pretooluse.sh", PRE_TOOL_USE_SCRIPT),
    ("codex-posttooluse.sh", POST_TOOL_USE_SCRIPT),
];

/// Events squeez registers under `hooks` in the Codex CLI hooks.json.
const SQUEEZ_EVENTS: [&str; 3] = ["SessionStart", "PreToolUse", "PostToolUse"];

/// Codex nests its event map under `hooks` and takes a per-hook timeout but no
/// entry name.
///
/// Command must be `bash <path>`, not a bare script path: Codex spawns the
/// command directly rather than handing it to a shell, so on Windows a bare
/// `.sh` path has no interpreter to run it and the hook dies immediately with
/// exit code 1 (every squeez Codex hook, starting with SessionStart).
///
/// On Windows even `bash <path>` cannot spawn (#278): Codex hands the command
/// to `cmd.exe`, where `bash` is rarely on PATH, and naming Git Bash by its
/// absolute path does not help — cmd's quote stripping mangles a command made
/// of two quoted paths (`"C:/Program Files/…/bash.exe" "…"`). So there the
/// command is a `<script>.cmd` launcher (see [`launcher_body`]) and nothing
/// else.
fn hook_specs(hooks_dir: &Path, windows: bool) -> Vec<settings_json::HookSpec> {
    let command = |script: &str| {
        if windows {
            launcher_command(&hooks_dir.join(format!("{script}.cmd")))
        } else {
            format!("bash {}", settings_json::shell_arg(&hooks_dir.join(script)))
        }
    };
    let spec = |event: &'static str, script: &'static str, timeout_ms: u64| settings_json::HookSpec {
        event,
        matcher: Some(".*"),
        command: command(script),
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

/// A Windows path with native separators — cmd.exe and batch files do not
/// reliably take `/`.
fn win_path(path: &Path) -> String {
    path.display().to_string().replace('/', "\\")
}

/// `<script>.cmd`: runs `script` under Git Bash, both named by absolute path,
/// and hands back its exit code. Stdin and stdout pass straight through, so
/// the hook payload and response are untouched. Named `<script>.cmd` (not
/// `<stem>.cmd`) so the command keeps the `/hooks/<script>` that marks it as
/// squeez's own.
fn launcher_body(bash: &Path, script: &Path) -> String {
    format!(
        "@echo off\r\n\"{}\" \"{}\"\r\nexit /b %ERRORLEVEL%\r\n",
        win_path(bash),
        win_path(script)
    )
}

/// One path is the only shape cmd.exe keeps intact: quoted when it has a
/// space (cmd leaves exactly two quotes around an existing file alone).
fn launcher_command(launcher: &Path) -> String {
    let path = win_path(launcher);
    if path.contains(' ') {
        format!("\"{path}\"")
    } else {
        path
    }
}

/// Git Bash, resolved when setup runs rather than left to the PATH of the
/// cmd.exe that Codex spawns: PATH first (skipping the WSL launchers), then
/// Git for Windows' default install locations.
fn git_bash() -> Option<PathBuf> {
    crate::commands::wrap::resolve_shell_program("bash")
        .map(PathBuf::from)
        .or_else(|| {
            let roots = ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"]
                .iter()
                .filter_map(|v| std::env::var_os(v).map(PathBuf::from))
                .chain(std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Programs")));
            roots
                .map(|r| r.join("Git").join("bin").join("bash.exe"))
                .find(|p| p.is_file())
        })
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

    fn registration(&self) -> settings_json::HookRegistration {
        settings_json::HookRegistration {
            path: Self::hooks_json_path(),
            root: settings_json::EventRoot::Nested,
            specs: hook_specs(&Self::hooks_dir(), cfg!(windows)),
        }
    }

    fn write_hook_scripts(hooks_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(hooks_dir)?;
        for (name, body) in HOOK_SCRIPTS {
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

    /// Fails rather than registering commands that cannot spawn: a missing
    /// interpreter used to surface only as Codex's generic hook failure.
    fn write_launchers(hooks_dir: &Path) -> std::io::Result<()> {
        let bash = git_bash().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Git Bash not found (PATH, %ProgramFiles%\\Git) — install Git for Windows \
                 or set CLAUDE_CODE_GIT_BASH_PATH, then re-run `squeez setup --host=codex`",
            )
        })?;
        for (name, _) in HOOK_SCRIPTS {
            let launcher = hooks_dir.join(format!("{name}.cmd"));
            std::fs::write(launcher, launcher_body(&bash, &hooks_dir.join(name)))?;
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
        Self::write_hook_scripts(&Self::hooks_dir())?;
        if cfg!(windows) {
            Self::write_launchers(&Self::hooks_dir())?;
        }
        settings_json::install_registration(&self.registration())
    }

    fn hook_registration(&self) -> Option<settings_json::HookRegistration> {
        Some(self.registration())
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
            if let Ok(file) = memory_file::open(&agents) {
                let cleaned = strip_squeez_block(file.existing());
                let _ = file.commit(&cleaned);
            }
        }
        Ok(())
    }

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
    fn strip_squeez_block_inside_existing_agents_md() {
        let s = "# repo rules\n<!-- squeez:start -->\nfoo\n<!-- squeez:end -->\n## after\n";
        let out = strip_squeez_block(s);
        assert!(!out.contains("<!-- squeez:start -->"));
        assert!(out.contains("# repo rules"));
        assert!(out.contains("## after"));
    }

    #[test]
    fn launcher_names_git_bash_and_the_script_by_absolute_path() {
        let body = launcher_body(
            Path::new("C:/Program Files/Git/bin/bash.exe"),
            Path::new("C:/Users/me/.codex/squeez/hooks/codex-pretooluse.sh"),
        );
        assert_eq!(
            body,
            "@echo off\r\n\
             \"C:\\Program Files\\Git\\bin\\bash.exe\" \"C:\\Users\\me\\.codex\\squeez\\hooks\\codex-pretooluse.sh\"\r\n\
             exit /b %ERRORLEVEL%\r\n"
        );
    }

    /// #278: on Windows the registered command is the launcher alone — no
    /// `bash` for cmd.exe to look up, no second quoted path for it to mangle —
    /// and it still carries `/hooks/<script>`, which is how setup, uninstall
    /// and doctor recognise squeez's own entries.
    #[test]
    fn windows_hook_commands_are_the_launchers() {
        let specs = hook_specs(Path::new("C:/Users/me/.codex/squeez/hooks"), true);
        for spec in &specs {
            assert_eq!(
                spec.command,
                format!("C:\\Users\\me\\.codex\\squeez\\hooks\\{}.cmd", spec.script)
            );
            assert!(settings_json::normalize_sep(&spec.command)
                .contains(&format!("/hooks/{}", spec.script)));
        }
    }

    #[test]
    fn a_launcher_path_with_a_space_is_quoted_once() {
        let specs = hook_specs(Path::new("C:/Users/First Last/.codex/squeez/hooks"), true);
        assert_eq!(
            specs[0].command,
            "\"C:\\Users\\First Last\\.codex\\squeez\\hooks\\codex-session-start.sh.cmd\""
        );
    }

    #[test]
    fn unix_hook_commands_stay_bash_plus_script() {
        let specs = hook_specs(Path::new("/h/.codex/squeez/hooks"), false);
        assert_eq!(specs[1].command, "bash \"/h/.codex/squeez/hooks/codex-pretooluse.sh\"");
    }
}
