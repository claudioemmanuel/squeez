use std::path::PathBuf;
use std::sync::Mutex;

use squeez::config::Config;
use squeez::hosts::{find, GeminiCliAdapter, HostAdapter, HostCaps};

// HOME is process-global; tests that mutate it must serialise.
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
    let path = std::env::temp_dir().join(format!("squeez-gemini-test-{uniq}"));
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

#[test]
fn gemini_capabilities_bash_wrap_session_mem_soft_budget() {
    let a = find("gemini").expect("gemini adapter");
    let caps = a.capabilities();
    assert!(caps.contains(HostCaps::BASH_WRAP));
    assert!(caps.contains(HostCaps::SESSION_MEM));
    assert!(caps.contains(HostCaps::BUDGET_SOFT));
    // BUDGET_HARD deliberately off — pending upstream schema docs.
    assert!(!caps.contains(HostCaps::BUDGET_HARD));
}

#[test]
fn gemini_data_dir_under_home_gemini_squeez() {
    with_home(|home| {
        let a = GeminiCliAdapter;
        assert_eq!(a.data_dir(), home.join(".gemini/squeez"));
    });
}

#[test]
fn gemini_inject_memory_writes_marker_block_with_soft_budget_hint() {
    with_home(|home| {
        let a = GeminiCliAdapter;
        let cfg = Config::default();
        a.inject_memory(&cfg, &[]).expect("inject");
        let gemini_md = home.join(".gemini/GEMINI.md");
        assert!(gemini_md.exists(), "GEMINI.md not created");
        let body = std::fs::read_to_string(&gemini_md).unwrap();
        assert!(body.contains("<!-- squeez:start -->"));
        assert!(body.contains("<!-- squeez:end -->"));
        assert!(body.contains("soft enforcement"));
    });
}

#[test]
fn gemini_inject_memory_is_idempotent() {
    with_home(|home| {
        let a = GeminiCliAdapter;
        let cfg = Config::default();
        a.inject_memory(&cfg, &[]).unwrap();
        a.inject_memory(&cfg, &[]).unwrap();
        let body = std::fs::read_to_string(home.join(".gemini/GEMINI.md")).unwrap();
        assert_eq!(
            body.matches("<!-- squeez:start -->").count(),
            1,
            "duplicate squeez block after re-run"
        );
    });
}

#[test]
fn gemini_inject_memory_preserves_existing_content() {
    with_home(|home| {
        std::fs::create_dir_all(home.join(".gemini")).unwrap();
        let gemini_md = home.join(".gemini/GEMINI.md");
        std::fs::write(&gemini_md, "# user rules\ndo X\n").unwrap();
        let a = GeminiCliAdapter;
        a.inject_memory(&Config::default(), &[]).unwrap();
        let body = std::fs::read_to_string(&gemini_md).unwrap();
        assert!(body.contains("<!-- squeez:start -->"));
        assert!(body.contains("# user rules"));
        assert!(body.contains("do X"));
    });
}

#[test]
fn gemini_install_writes_hook_scripts_and_patches_settings() {
    // install() shells out to python3; skip gracefully if python3 is absent
    // on the test host (we cannot mock it portably without a crate).
    if std::process::Command::new("python3")
        .arg("--version")
        .status()
        .map(|s| !s.success())
        .unwrap_or(true)
    {
        eprintln!("python3 unavailable — skipping install test");
        return;
    }
    with_home(|home| {
        let a = GeminiCliAdapter;
        a.install(&PathBuf::from("/usr/local/bin/squeez"))
            .expect("install");
        let hooks_dir = home.join(".gemini/squeez/hooks");
        for script in [
            "gemini-session-start.sh",
            "gemini-before-tool.sh",
            "gemini-after-tool.sh",
        ] {
            assert!(hooks_dir.join(script).exists(), "{} missing", script);
        }
        let settings = home.join(".gemini/settings.json");
        assert!(settings.exists(), "settings.json not written");
        let body = std::fs::read_to_string(&settings).unwrap();
        assert!(body.contains("SessionStart"));
        assert!(body.contains("BeforeTool"));
        assert!(body.contains("AfterTool"));
        assert!(body.contains("gemini-before-tool.sh"));
    });
}

#[test]
fn gemini_install_preserves_existing_settings() {
    if std::process::Command::new("python3")
        .arg("--version")
        .status()
        .map(|s| !s.success())
        .unwrap_or(true)
    {
        return;
    }
    with_home(|home| {
        std::fs::create_dir_all(home.join(".gemini")).unwrap();
        // Seed settings.json with an unrelated key.
        std::fs::write(
            home.join(".gemini/settings.json"),
            r#"{"theme": "dracula", "other": {"foo": 1}}"#,
        )
        .unwrap();
        let a = GeminiCliAdapter;
        a.install(&PathBuf::from("/usr/local/bin/squeez")).unwrap();
        let body = std::fs::read_to_string(home.join(".gemini/settings.json")).unwrap();
        assert!(body.contains("dracula"), "unrelated 'theme' key dropped");
        assert!(body.contains("BeforeTool"));
    });
}

#[test]
fn gemini_install_is_idempotent_no_duplicate_entries() {
    if std::process::Command::new("python3")
        .arg("--version")
        .status()
        .map(|s| !s.success())
        .unwrap_or(true)
    {
        return;
    }
    with_home(|home| {
        let a = GeminiCliAdapter;
        a.install(&PathBuf::from("/usr/local/bin/squeez")).unwrap();
        a.install(&PathBuf::from("/usr/local/bin/squeez")).unwrap();
        let body = std::fs::read_to_string(home.join(".gemini/settings.json")).unwrap();
        // Should have one SessionStart matcher, one BeforeTool, one AfterTool.
        let count_session = body.matches("gemini-session-start.sh").count();
        assert_eq!(count_session, 1, "duplicate SessionStart entries: {body}");
    });
}

#[test]
fn gemini_uninstall_removes_hooks_and_strips_memory_block() {
    if std::process::Command::new("python3")
        .arg("--version")
        .status()
        .map(|s| !s.success())
        .unwrap_or(true)
    {
        return;
    }
    with_home(|home| {
        let a = GeminiCliAdapter;
        a.install(&PathBuf::from("/usr/local/bin/squeez")).unwrap();
        a.inject_memory(&Config::default(), &[]).unwrap();
        a.uninstall().unwrap();

        // Hook scripts directory gone
        assert!(!home.join(".gemini/squeez/hooks").exists());
        // settings.json still parseable, squeez entries stripped
        let body = std::fs::read_to_string(home.join(".gemini/settings.json")).unwrap();
        assert!(!body.contains("gemini-session-start.sh"));
        // GEMINI.md still there, squeez block removed
        let md = std::fs::read_to_string(home.join(".gemini/GEMINI.md")).unwrap();
        assert!(!md.contains("<!-- squeez:start -->"));
    });
}
