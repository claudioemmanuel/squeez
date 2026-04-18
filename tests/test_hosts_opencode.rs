use std::path::PathBuf;
use std::sync::Mutex;

use squeez::config::Config;
use squeez::hosts::{find, HostCaps, OpenCodeAdapter};
use squeez::hosts::HostAdapter;

// XDG_CONFIG_HOME is process-global — serialise tests that mutate it so
// parallel `cargo test` doesn't race.
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
    let path = std::env::temp_dir().join(format!("squeez-opencode-test-{uniq}"));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn with_xdg<F: FnOnce(&PathBuf) -> R, R>(f: F) -> R {
    let guard = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let home = tmp_home();
    std::env::set_var("XDG_CONFIG_HOME", &home);
    let r = f(&home);
    std::env::remove_var("XDG_CONFIG_HOME");
    drop(guard);
    r
}

#[test]
fn opencode_capabilities_full_parity() {
    let a = find("opencode").expect("opencode adapter");
    let caps = a.capabilities();
    assert!(caps.contains(HostCaps::BASH_WRAP));
    assert!(caps.contains(HostCaps::SESSION_MEM));
    assert!(caps.contains(HostCaps::BUDGET_HARD));
}

#[test]
fn opencode_data_dir_respects_xdg_config_home() {
    with_xdg(|home| {
        let a = OpenCodeAdapter;
        let d = a.data_dir();
        assert!(d.starts_with(home), "data_dir {:?} not under {:?}", d, home);
        assert!(d.ends_with("opencode/squeez"), "unexpected suffix: {:?}", d);
    });
}

#[test]
fn opencode_install_drops_plugin_file() {
    with_xdg(|home| {
        let a = OpenCodeAdapter;
        a.install(&PathBuf::from("/usr/local/bin/squeez"))
            .expect("install should succeed");
        let plugin = home.join("opencode/plugins/squeez.js");
        assert!(plugin.exists(), "plugin file missing at {:?}", plugin);
        let body = std::fs::read_to_string(&plugin).unwrap();
        assert!(body.contains("SqueezPlugin"));
        assert!(body.contains("tool.execute.before"));
        assert!(body.contains("session.created"));
    });
}

#[test]
fn opencode_inject_memory_writes_marker_block() {
    with_xdg(|home| {
        let a = OpenCodeAdapter;
        let cfg = Config::default();
        a.inject_memory(&cfg, &[]).expect("inject_memory should succeed");
        let agents = home.join("opencode/AGENTS.md");
        assert!(agents.exists(), "AGENTS.md not created");
        let body = std::fs::read_to_string(&agents).unwrap();
        assert!(body.contains("<!-- squeez:start -->"));
        assert!(body.contains("<!-- squeez:end -->"));
        assert!(body.contains("squeez — session context"));
    });
}

#[test]
fn opencode_inject_memory_is_idempotent() {
    with_xdg(|home| {
        let a = OpenCodeAdapter;
        let cfg = Config::default();
        a.inject_memory(&cfg, &[]).expect("first run");
        a.inject_memory(&cfg, &[]).expect("second run");
        let agents = home.join("opencode/AGENTS.md");
        let body = std::fs::read_to_string(&agents).unwrap();
        assert_eq!(
            body.matches("<!-- squeez:start -->").count(),
            1,
            "duplicate squeez block after idempotent re-run"
        );
    });
}

#[test]
fn opencode_inject_memory_preserves_existing_content() {
    with_xdg(|home| {
        std::fs::create_dir_all(home.join("opencode")).unwrap();
        let agents_path = home.join("opencode/AGENTS.md");
        std::fs::write(&agents_path, "# My existing rules\nuse tabs\n").unwrap();
        let a = OpenCodeAdapter;
        let cfg = Config::default();
        a.inject_memory(&cfg, &[]).expect("inject");
        let body = std::fs::read_to_string(&agents_path).unwrap();
        assert!(body.contains("<!-- squeez:start -->"));
        assert!(body.contains("# My existing rules"));
        assert!(body.contains("use tabs"));
    });
}

#[test]
fn opencode_uninstall_strips_marker_block_and_removes_plugin() {
    with_xdg(|home| {
        let a = OpenCodeAdapter;
        a.install(&PathBuf::from("/usr/local/bin/squeez")).unwrap();
        a.inject_memory(&Config::default(), &[]).unwrap();
        let plugin = home.join("opencode/plugins/squeez.js");
        let agents = home.join("opencode/AGENTS.md");
        assert!(plugin.exists());
        assert!(agents.exists());
        a.uninstall().unwrap();
        assert!(!plugin.exists(), "plugin should be removed");
        let body = std::fs::read_to_string(&agents).unwrap();
        assert!(!body.contains("<!-- squeez:start -->"));
    });
}
