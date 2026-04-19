use std::path::PathBuf;
use std::sync::Mutex;

use squeez::config::Config;
use squeez::hosts::HostAdapter;
use squeez::hosts::{
    ClaudeCodeAdapter, CodexCliAdapter, CopilotCliAdapter, GeminiCliAdapter, OpenCodeAdapter,
};

// HOME and XDG_CONFIG_HOME are process-global — serialise every test that
// mutates them so parallel `cargo test` threads don't race.
static ENV_GUARD: Mutex<()> = Mutex::new(());

/// Create a unique temp directory and return its path.
fn tmp_dir(tag: &str) -> PathBuf {
    let uniq = format!(
        "{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        std::process::id()
    );
    let path = std::env::temp_dir().join(format!("squeez-memwarn-{tag}-{uniq}"));
    std::fs::create_dir_all(&path).unwrap();
    path
}

// ── helpers ────────────────────────────────────────────────────────────────

fn cfg_default() -> Config {
    Config::default() // memory_file_warn_tokens = 1000
}

fn cfg_disabled() -> Config {
    let mut c = Config::default();
    c.memory_file_warn_tokens = 0;
    c
}

// ── ClaudeCodeAdapter ──────────────────────────────────────────────────────

#[test]
fn claude_code_warns_when_file_exceeds_threshold() {
    let _g = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let home = tmp_dir("cc-large");
    std::env::set_var("HOME", &home);
    std::env::remove_var("SQUEEZ_DIR");
    std::env::remove_var("XDG_CONFIG_HOME");

    // Write 4000 non-squeez chars to CLAUDE.md (1000 tokens, threshold is 1000 so need >1000)
    let claude_dir = home.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let claude_md = claude_dir.join("CLAUDE.md");
    std::fs::write(&claude_md, "a".repeat(4004)).unwrap(); // 1001 tokens > 1000

    let a = ClaudeCodeAdapter;
    a.inject_memory(&cfg_default(), &[]).expect("inject_memory");

    let body = std::fs::read_to_string(&claude_md).unwrap();
    assert!(
        body.contains("tokens"),
        "expected size-warning banner in CLAUDE.md, got: {}",
        &body[..body.len().min(300)]
    );
    assert!(
        body.contains("CLAUDE.md"),
        "banner should name the file"
    );

    std::env::remove_var("HOME");
}

#[test]
fn claude_code_no_warn_when_file_under_threshold() {
    let _g = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let home = tmp_dir("cc-small");
    std::env::set_var("HOME", &home);
    std::env::remove_var("SQUEEZ_DIR");
    std::env::remove_var("XDG_CONFIG_HOME");

    let claude_dir = home.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let claude_md = claude_dir.join("CLAUDE.md");
    std::fs::write(&claude_md, "a".repeat(500)).unwrap(); // 125 tokens

    let a = ClaudeCodeAdapter;
    a.inject_memory(&cfg_default(), &[]).expect("inject_memory");

    let body = std::fs::read_to_string(&claude_md).unwrap();
    // The squeez block's own "tokens" text should not appear as a warning
    // — the banner format is "~N tokens (> LIMIT recommended)"
    assert!(
        !body.contains("recommended"),
        "unexpected warning banner for small file"
    );

    std::env::remove_var("HOME");
}

#[test]
fn claude_code_no_warn_when_limit_is_zero() {
    let _g = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let home = tmp_dir("cc-zero");
    std::env::set_var("HOME", &home);
    std::env::remove_var("SQUEEZ_DIR");
    std::env::remove_var("XDG_CONFIG_HOME");

    let claude_dir = home.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let claude_md = claude_dir.join("CLAUDE.md");
    std::fs::write(&claude_md, "a".repeat(4004)).unwrap();

    let a = ClaudeCodeAdapter;
    a.inject_memory(&cfg_disabled(), &[]).expect("inject_memory");

    let body = std::fs::read_to_string(&claude_md).unwrap();
    assert!(
        !body.contains("recommended"),
        "warning should be suppressed when memory_file_warn_tokens=0"
    );

    std::env::remove_var("HOME");
}

// ── CopilotCliAdapter ──────────────────────────────────────────────────────

#[test]
fn copilot_warns_when_file_exceeds_threshold() {
    let _g = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let home = tmp_dir("cop-large");
    std::env::set_var("HOME", &home);
    std::env::remove_var("SQUEEZ_DIR");
    std::env::remove_var("XDG_CONFIG_HOME");

    let copilot_dir = home.join(".copilot");
    std::fs::create_dir_all(&copilot_dir).unwrap();
    let instructions = copilot_dir.join("copilot-instructions.md");
    std::fs::write(&instructions, "a".repeat(4004)).unwrap();

    let a = CopilotCliAdapter;
    a.inject_memory(&cfg_default(), &[]).expect("inject_memory");

    let body = std::fs::read_to_string(&instructions).unwrap();
    assert!(body.contains("tokens"), "expected warning banner");
    assert!(body.contains("copilot-instructions.md"), "banner should name the file");

    std::env::remove_var("HOME");
}

#[test]
fn copilot_no_warn_when_file_under_threshold() {
    let _g = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let home = tmp_dir("cop-small");
    std::env::set_var("HOME", &home);
    std::env::remove_var("SQUEEZ_DIR");
    std::env::remove_var("XDG_CONFIG_HOME");

    let copilot_dir = home.join(".copilot");
    std::fs::create_dir_all(&copilot_dir).unwrap();
    let instructions = copilot_dir.join("copilot-instructions.md");
    std::fs::write(&instructions, "a".repeat(500)).unwrap();

    let a = CopilotCliAdapter;
    a.inject_memory(&cfg_default(), &[]).expect("inject_memory");

    let body = std::fs::read_to_string(&instructions).unwrap();
    assert!(!body.contains("recommended"), "unexpected warning for small file");

    std::env::remove_var("HOME");
}

#[test]
fn copilot_no_warn_when_limit_is_zero() {
    let _g = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let home = tmp_dir("cop-zero");
    std::env::set_var("HOME", &home);
    std::env::remove_var("SQUEEZ_DIR");
    std::env::remove_var("XDG_CONFIG_HOME");

    let copilot_dir = home.join(".copilot");
    std::fs::create_dir_all(&copilot_dir).unwrap();
    let instructions = copilot_dir.join("copilot-instructions.md");
    std::fs::write(&instructions, "a".repeat(4004)).unwrap();

    let a = CopilotCliAdapter;
    a.inject_memory(&cfg_disabled(), &[]).expect("inject_memory");

    let body = std::fs::read_to_string(&instructions).unwrap();
    assert!(!body.contains("recommended"), "warning suppressed when limit=0");

    std::env::remove_var("HOME");
}

// ── OpenCodeAdapter ────────────────────────────────────────────────────────

#[test]
fn opencode_warns_when_file_exceeds_threshold() {
    let _g = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let home = tmp_dir("oc-large");
    std::env::set_var("XDG_CONFIG_HOME", &home);
    std::env::remove_var("SQUEEZ_DIR");

    let oc_dir = home.join("opencode");
    std::fs::create_dir_all(&oc_dir).unwrap();
    let agents_md = oc_dir.join("AGENTS.md");
    std::fs::write(&agents_md, "a".repeat(4004)).unwrap();

    let a = OpenCodeAdapter;
    a.inject_memory(&cfg_default(), &[]).expect("inject_memory");

    let body = std::fs::read_to_string(&agents_md).unwrap();
    assert!(body.contains("tokens"), "expected warning banner");
    assert!(body.contains("AGENTS.md"), "banner should name the file");

    std::env::remove_var("XDG_CONFIG_HOME");
}

#[test]
fn opencode_no_warn_when_file_under_threshold() {
    let _g = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let home = tmp_dir("oc-small");
    std::env::set_var("XDG_CONFIG_HOME", &home);
    std::env::remove_var("SQUEEZ_DIR");

    let oc_dir = home.join("opencode");
    std::fs::create_dir_all(&oc_dir).unwrap();
    let agents_md = oc_dir.join("AGENTS.md");
    std::fs::write(&agents_md, "a".repeat(500)).unwrap();

    let a = OpenCodeAdapter;
    a.inject_memory(&cfg_default(), &[]).expect("inject_memory");

    let body = std::fs::read_to_string(&agents_md).unwrap();
    assert!(!body.contains("recommended"), "unexpected warning for small file");

    std::env::remove_var("XDG_CONFIG_HOME");
}

#[test]
fn opencode_no_warn_when_limit_is_zero() {
    let _g = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let home = tmp_dir("oc-zero");
    std::env::set_var("XDG_CONFIG_HOME", &home);
    std::env::remove_var("SQUEEZ_DIR");

    let oc_dir = home.join("opencode");
    std::fs::create_dir_all(&oc_dir).unwrap();
    let agents_md = oc_dir.join("AGENTS.md");
    std::fs::write(&agents_md, "a".repeat(4004)).unwrap();

    let a = OpenCodeAdapter;
    a.inject_memory(&cfg_disabled(), &[]).expect("inject_memory");

    let body = std::fs::read_to_string(&agents_md).unwrap();
    assert!(!body.contains("recommended"), "warning suppressed when limit=0");

    std::env::remove_var("XDG_CONFIG_HOME");
}

// ── GeminiCliAdapter ───────────────────────────────────────────────────────

#[test]
fn gemini_warns_when_file_exceeds_threshold() {
    let _g = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let home = tmp_dir("gem-large");
    std::env::set_var("HOME", &home);
    std::env::remove_var("SQUEEZ_DIR");
    std::env::remove_var("XDG_CONFIG_HOME");

    let gemini_dir = home.join(".gemini");
    std::fs::create_dir_all(&gemini_dir).unwrap();
    let gemini_md = gemini_dir.join("GEMINI.md");
    std::fs::write(&gemini_md, "a".repeat(4004)).unwrap();

    let a = GeminiCliAdapter;
    a.inject_memory(&cfg_default(), &[]).expect("inject_memory");

    let body = std::fs::read_to_string(&gemini_md).unwrap();
    assert!(body.contains("tokens"), "expected warning banner");
    assert!(body.contains("GEMINI.md"), "banner should name the file");

    std::env::remove_var("HOME");
}

#[test]
fn gemini_no_warn_when_file_under_threshold() {
    let _g = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let home = tmp_dir("gem-small");
    std::env::set_var("HOME", &home);
    std::env::remove_var("SQUEEZ_DIR");
    std::env::remove_var("XDG_CONFIG_HOME");

    let gemini_dir = home.join(".gemini");
    std::fs::create_dir_all(&gemini_dir).unwrap();
    let gemini_md = gemini_dir.join("GEMINI.md");
    std::fs::write(&gemini_md, "a".repeat(500)).unwrap();

    let a = GeminiCliAdapter;
    a.inject_memory(&cfg_default(), &[]).expect("inject_memory");

    let body = std::fs::read_to_string(&gemini_md).unwrap();
    assert!(!body.contains("recommended"), "unexpected warning for small file");

    std::env::remove_var("HOME");
}

#[test]
fn gemini_no_warn_when_limit_is_zero() {
    let _g = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let home = tmp_dir("gem-zero");
    std::env::set_var("HOME", &home);
    std::env::remove_var("SQUEEZ_DIR");
    std::env::remove_var("XDG_CONFIG_HOME");

    let gemini_dir = home.join(".gemini");
    std::fs::create_dir_all(&gemini_dir).unwrap();
    let gemini_md = gemini_dir.join("GEMINI.md");
    std::fs::write(&gemini_md, "a".repeat(4004)).unwrap();

    let a = GeminiCliAdapter;
    a.inject_memory(&cfg_disabled(), &[]).expect("inject_memory");

    let body = std::fs::read_to_string(&gemini_md).unwrap();
    assert!(!body.contains("recommended"), "warning suppressed when limit=0");

    std::env::remove_var("HOME");
}

// ── CodexCliAdapter ────────────────────────────────────────────────────────

#[test]
fn codex_warns_when_file_exceeds_threshold() {
    let _g = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let home = tmp_dir("cdx-large");
    std::env::set_var("HOME", &home);
    std::env::remove_var("SQUEEZ_DIR");
    std::env::remove_var("XDG_CONFIG_HOME");

    let codex_dir = home.join(".codex");
    std::fs::create_dir_all(&codex_dir).unwrap();
    let agents_md = codex_dir.join("AGENTS.md");
    std::fs::write(&agents_md, "a".repeat(4004)).unwrap();

    let a = CodexCliAdapter;
    a.inject_memory(&cfg_default(), &[]).expect("inject_memory");

    let body = std::fs::read_to_string(&agents_md).unwrap();
    assert!(body.contains("tokens"), "expected warning banner");
    assert!(body.contains("AGENTS.md"), "banner should name the file");

    std::env::remove_var("HOME");
}

#[test]
fn codex_no_warn_when_file_under_threshold() {
    let _g = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let home = tmp_dir("cdx-small");
    std::env::set_var("HOME", &home);
    std::env::remove_var("SQUEEZ_DIR");
    std::env::remove_var("XDG_CONFIG_HOME");

    let codex_dir = home.join(".codex");
    std::fs::create_dir_all(&codex_dir).unwrap();
    let agents_md = codex_dir.join("AGENTS.md");
    std::fs::write(&agents_md, "a".repeat(500)).unwrap();

    let a = CodexCliAdapter;
    a.inject_memory(&cfg_default(), &[]).expect("inject_memory");

    let body = std::fs::read_to_string(&agents_md).unwrap();
    assert!(!body.contains("recommended"), "unexpected warning for small file");

    std::env::remove_var("HOME");
}

#[test]
fn codex_no_warn_when_limit_is_zero() {
    let _g = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let home = tmp_dir("cdx-zero");
    std::env::set_var("HOME", &home);
    std::env::remove_var("SQUEEZ_DIR");
    std::env::remove_var("XDG_CONFIG_HOME");

    let codex_dir = home.join(".codex");
    std::fs::create_dir_all(&codex_dir).unwrap();
    let agents_md = codex_dir.join("AGENTS.md");
    std::fs::write(&agents_md, "a".repeat(4004)).unwrap();

    let a = CodexCliAdapter;
    a.inject_memory(&cfg_disabled(), &[]).expect("inject_memory");

    let body = std::fs::read_to_string(&agents_md).unwrap();
    assert!(!body.contains("recommended"), "warning suppressed when limit=0");

    std::env::remove_var("HOME");
}
