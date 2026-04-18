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
