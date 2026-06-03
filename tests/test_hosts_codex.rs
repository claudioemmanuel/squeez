use std::path::PathBuf;
use std::sync::Mutex;

use squeez::config::Config;
use squeez::hosts::{find, CodexCliAdapter, HostAdapter, HostCaps};

static ENV_GUARD: Mutex<()> = Mutex::new(());

fn tmp_home() -> PathBuf {
    let uniq = format!(
        "{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        std::process::id()
    );
    let path = std::env::temp_dir().join(format!("squeez-codex-test-{uniq}"));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn with_home<F: FnOnce(&PathBuf) -> R, R>(f: F) -> R {
    let guard = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let home = tmp_home();
    let prev_home = std::env::var("HOME").ok();
    let prev_userprofile = std::env::var("USERPROFILE").ok();
    std::env::set_var("HOME", &home);
    std::env::remove_var("USERPROFILE");
    let r = f(&home);
    if let Some(h) = prev_home {
        std::env::set_var("HOME", h);
    } else {
        std::env::remove_var("HOME");
    }
    if let Some(u) = prev_userprofile {
        std::env::set_var("USERPROFILE", u);
    }
    drop(guard);
    r
}

fn python3_missing() -> bool {
    std::process::Command::new("python3")
        .arg("--version")
        .status()
        .map(|s| !s.success())
        .unwrap_or(true)
}

#[test]
fn codex_capabilities_bash_wrap_session_mem_soft_budget() {
    let a = find("codex").expect("codex adapter");
    let caps = a.capabilities();
    assert!(caps.contains(HostCaps::BASH_WRAP));
    assert!(caps.contains(HostCaps::SESSION_MEM));
    assert!(caps.contains(HostCaps::BUDGET_SOFT));
    assert!(!caps.contains(HostCaps::BUDGET_HARD));
}

#[test]
fn codex_data_dir_under_home_codex_squeez() {
    with_home(|home| {
        let a = CodexCliAdapter;
        assert_eq!(a.data_dir(), home.join(".codex/squeez"));
    });
}

#[test]
fn codex_inject_memory_writes_block_with_soft_budget_hints() {
    with_home(|home| {
        let a = CodexCliAdapter;
        a.inject_memory(&Config::default(), &[]).unwrap();
        let agents = home.join(".codex/AGENTS.md");
        assert!(agents.exists(), "AGENTS.md not created");
        let body = std::fs::read_to_string(&agents).unwrap();
        assert!(body.contains("<!-- squeez:start -->"));
        assert!(body.contains("<!-- squeez:end -->"));
        assert!(body.contains("soft enforcement"));
        assert!(body.contains("read_file"));
        assert!(body.contains("apply_patch"));
    });
}

#[test]
fn codex_inject_memory_idempotent() {
    with_home(|home| {
        let a = CodexCliAdapter;
        a.inject_memory(&Config::default(), &[]).unwrap();
        a.inject_memory(&Config::default(), &[]).unwrap();
        let body = std::fs::read_to_string(home.join(".codex/AGENTS.md")).unwrap();
        assert_eq!(body.matches("<!-- squeez:start -->").count(), 1);
    });
}

#[test]
fn codex_inject_memory_preserves_existing_content() {
    with_home(|home| {
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        let agents = home.join(".codex/AGENTS.md");
        std::fs::write(&agents, "# existing rules\nuse 2-space indent\n").unwrap();
        CodexCliAdapter
            .inject_memory(&Config::default(), &[])
            .unwrap();
        let body = std::fs::read_to_string(&agents).unwrap();
        assert!(body.contains("<!-- squeez:start -->"));
        assert!(body.contains("# existing rules"));
        assert!(body.contains("use 2-space indent"));
    });
}

#[test]
fn codex_install_writes_hooks_and_patches_hooks_json() {
    if python3_missing() {
        return;
    }
    with_home(|home| {
        let a = CodexCliAdapter;
        a.install(&PathBuf::from("/usr/local/bin/squeez"))
            .expect("install");
        let hooks_dir = home.join(".codex/squeez/hooks");
        for script in [
            "codex-session-start.sh",
            "codex-pretooluse.sh",
            "codex-posttooluse.sh",
        ] {
            assert!(hooks_dir.join(script).exists(), "{} missing", script);
        }
        let hooks_json = home.join(".codex/hooks.json");
        assert!(hooks_json.exists());
        let body = std::fs::read_to_string(&hooks_json).unwrap();
        assert!(body.contains("SessionStart"));
        assert!(body.contains("PreToolUse"));
        assert!(body.contains("PostToolUse"));
        assert!(body.contains("codex-pretooluse.sh"));
    });
}

#[test]
fn codex_install_preserves_existing_hooks_json_keys() {
    if python3_missing() {
        return;
    }
    with_home(|home| {
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::write(
            home.join(".codex/hooks.json"),
            r#"{"user_field": 42, "hooks": {"PreToolUse": [{"matcher": "special", "hooks": [{"type": "command", "command": "/tmp/mine.sh"}]}]}}"#,
        )
        .unwrap();
        CodexCliAdapter
            .install(&PathBuf::from("/usr/local/bin/squeez"))
            .unwrap();
        let body = std::fs::read_to_string(home.join(".codex/hooks.json")).unwrap();
        assert!(body.contains("user_field"), "unrelated key dropped");
        assert!(body.contains("/tmp/mine.sh"), "existing hook dropped");
        assert!(body.contains("codex-pretooluse.sh"), "squeez hook missing");
    });
}

#[test]
fn codex_install_idempotent_no_dup_entries() {
    if python3_missing() {
        return;
    }
    with_home(|home| {
        let a = CodexCliAdapter;
        a.install(&PathBuf::from("/usr/local/bin/squeez")).unwrap();
        a.install(&PathBuf::from("/usr/local/bin/squeez")).unwrap();
        let body = std::fs::read_to_string(home.join(".codex/hooks.json")).unwrap();
        assert_eq!(body.matches("codex-session-start.sh").count(), 1);
        assert_eq!(body.matches("codex-pretooluse.sh").count(), 1);
    });
}

/// Create a fake executable `squeez` under <home>/.claude/squeez/bin so the
/// hook scripts resolve `$SQUEEZ` to a real path. PreToolUse never executes it
/// (it only string-builds the rewritten command), so a stub is enough.
fn fake_squeez(home: &PathBuf) -> PathBuf {
    let bin = home.join(".claude/squeez/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let p = bin.join("squeez");
    std::fs::write(&p, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    p
}

fn run_hook(script: &str, home: &PathBuf, stdin: &str) -> std::process::Output {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("bash")
        .arg(format!("hooks/{script}"))
        .env("HOME", home)
        .env_remove("USERPROFILE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn hook");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn codex_pretooluse_emits_current_hookspecificoutput_shape() {
    if python3_missing() {
        return;
    }
    with_home(|home| {
        fake_squeez(home);
        let payload = r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"ls -la"}}"#;
        let out = run_hook("codex-pretooluse.sh", home, payload);
        assert!(out.status.success(), "hook should exit 0");
        let stdout = String::from_utf8_lossy(&out.stdout);
        // Current Codex schema: nested hookSpecificOutput / permissionDecision.
        assert!(stdout.contains("hookSpecificOutput"), "got: {stdout}");
        assert!(stdout.contains("\"hookEventName\": \"PreToolUse\"") || stdout.contains("\"hookEventName\":\"PreToolUse\""), "got: {stdout}");
        assert!(stdout.contains("\"permissionDecision\": \"allow\"") || stdout.contains("\"permissionDecision\":\"allow\""), "got: {stdout}");
        assert!(stdout.contains("updatedInput"), "got: {stdout}");
        assert!(stdout.contains("wrap"), "command should be wrapped, got: {stdout}");
        // Must NOT use the stale top-level decision shape.
        assert!(!stdout.contains("\"decision\""), "stale shape leaked: {stdout}");
    });
}

#[test]
fn codex_pretooluse_ignores_non_shell_tool() {
    if python3_missing() {
        return;
    }
    with_home(|home| {
        fake_squeez(home);
        let payload = r#"{"hook_event_name":"PreToolUse","tool_name":"read_file","tool_input":{"path":"/etc/hosts"}}"#;
        let out = run_hook("codex-pretooluse.sh", home, payload);
        assert!(out.status.success());
        assert!(out.stdout.is_empty(), "non-shell tool must produce no rewrite");
    });
}

#[test]
fn codex_posttooluse_is_silent_on_success() {
    if python3_missing() {
        return;
    }
    with_home(|home| {
        fake_squeez(home);
        let payload = r#"{"tool_name":"Bash","tool_result":{"content":"ok"}}"#;
        let out = run_hook("codex-posttooluse.sh", home, payload);
        assert!(out.status.success(), "tracking hook should exit 0");
        assert!(out.stdout.is_empty(), "tracking hook must emit no stdout, got: {:?}", String::from_utf8_lossy(&out.stdout));
    });
}

#[test]
fn codex_session_start_wraps_context_when_present() {
    if python3_missing() {
        return;
    }
    with_home(|home| {
        fake_squeez(home);
        let out = run_hook("codex-session-start.sh", home, "");
        assert!(out.status.success());
        let stdout = String::from_utf8_lossy(&out.stdout);
        // The stub squeez emits no init output, so the hook stays silent.
        // If anything is emitted it must be the documented SessionStart shape.
        if !stdout.trim().is_empty() {
            assert!(stdout.contains("hookSpecificOutput"), "got: {stdout}");
            assert!(stdout.contains("additionalContext"), "got: {stdout}");
        }
    });
}

#[test]
fn codex_uninstall_removes_hooks_and_strips_memory_block() {
    if python3_missing() {
        return;
    }
    with_home(|home| {
        let a = CodexCliAdapter;
        a.install(&PathBuf::from("/usr/local/bin/squeez")).unwrap();
        a.inject_memory(&Config::default(), &[]).unwrap();
        a.uninstall().unwrap();

        assert!(!home.join(".codex/squeez/hooks").exists());
        let body = std::fs::read_to_string(home.join(".codex/hooks.json")).unwrap();
        assert!(!body.contains("codex-pretooluse.sh"));
        let md = std::fs::read_to_string(home.join(".codex/AGENTS.md")).unwrap();
        assert!(!md.contains("<!-- squeez:start -->"));
    });
}
