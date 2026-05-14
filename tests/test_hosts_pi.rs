use std::path::PathBuf;
use std::sync::Mutex;

use squeez::config::Config;
use squeez::hosts::{find, PiAdapter, HostAdapter, HostCaps};

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
    let path = std::env::temp_dir().join(format!("squeez-pi-test-{uniq}"));
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
fn pi_capabilities_bash_wrap_session_mem_budget_hard() {
    let a = find("pi").expect("pi adapter");
    let caps = a.capabilities();
    assert!(caps.contains(HostCaps::BASH_WRAP));
    assert!(caps.contains(HostCaps::SESSION_MEM));
    assert!(caps.contains(HostCaps::BUDGET_HARD));
    assert!(!caps.contains(HostCaps::BUDGET_SOFT));
}

#[test]
fn pi_data_dir_under_home_pi_agent_squeez() {
    with_home(|home| {
        let a = PiAdapter;
        assert_eq!(a.data_dir(), home.join(".pi/agent/squeez"));
    });
}

#[test]
fn pi_is_installed_when_pi_agent_dir_exists() {
    with_home(|home| {
        let a = PiAdapter;
        assert!(!a.is_installed(), "should not detect Pi before dir exists");
        std::fs::create_dir_all(home.join(".pi/agent")).unwrap();
        assert!(a.is_installed(), "should detect Pi after dir created");
    });
}

#[test]
fn pi_install_writes_extension_and_skill() {
    with_home(|home| {
        std::fs::create_dir_all(home.join(".pi/agent")).unwrap();
        let a = PiAdapter;
        a.install(&PathBuf::from("/usr/local/bin/squeez"))
            .expect("install");
        let ext = home.join(".pi/agent/extensions/squeez/index.ts");
        assert!(ext.exists(), "extension not written");
        let body = std::fs::read_to_string(&ext).unwrap();
        assert!(body.contains("squeez wrap"), "extension missing wrap logic");
        assert!(body.contains("tool_result"), "extension missing tool_result handler");
        let skill = home.join(".pi/agent/skills/squeez/SKILL.md");
        assert!(skill.exists(), "SKILL.md not written");
        let skill_body = std::fs::read_to_string(&skill).unwrap();
        assert!(skill_body.contains("name: squeez"));
    });
}

#[test]
fn pi_install_is_idempotent() {
    with_home(|home| {
        std::fs::create_dir_all(home.join(".pi/agent")).unwrap();
        let a = PiAdapter;
        a.install(&PathBuf::from("/usr/local/bin/squeez")).unwrap();
        a.install(&PathBuf::from("/usr/local/bin/squeez")).unwrap();
        let ext = home.join(".pi/agent/extensions/squeez/index.ts");
        assert!(ext.exists());
    });
}

#[test]
fn pi_uninstall_removes_extension_and_skill() {
    with_home(|home| {
        std::fs::create_dir_all(home.join(".pi/agent")).unwrap();
        let a = PiAdapter;
        a.install(&PathBuf::from("/usr/local/bin/squeez")).unwrap();
        a.inject_memory(&Config::default(), &[]).unwrap();
        a.uninstall().unwrap();
        assert!(!home.join(".pi/agent/extensions/squeez").exists());
        assert!(!home.join(".pi/agent/skills/squeez").exists());
    });
}

#[test]
fn pi_inject_memory_writes_marker_block() {
    with_home(|home| {
        std::fs::create_dir_all(home.join(".pi/agent")).unwrap();
        let a = PiAdapter;
        a.inject_memory(&Config::default(), &[]).expect("inject");
        let skill = home.join(".pi/agent/skills/squeez/SKILL.md");
        assert!(skill.exists(), "SKILL.md not created");
        let body = std::fs::read_to_string(&skill).unwrap();
        assert!(body.contains("<!-- squeez:start -->"));
        assert!(body.contains("<!-- squeez:end -->"));
        assert!(body.contains("name: squeez"), "frontmatter missing");
    });
}

#[test]
fn pi_inject_memory_is_idempotent() {
    with_home(|home| {
        std::fs::create_dir_all(home.join(".pi/agent")).unwrap();
        let a = PiAdapter;
        a.inject_memory(&Config::default(), &[]).unwrap();
        a.inject_memory(&Config::default(), &[]).unwrap();
        let body = std::fs::read_to_string(
            home.join(".pi/agent/skills/squeez/SKILL.md"),
        )
        .unwrap();
        assert_eq!(
            body.matches("<!-- squeez:start -->").count(),
            1,
            "duplicate squeez block after re-run"
        );
    });
}

#[test]
fn pi_inject_memory_preserves_frontmatter() {
    with_home(|home| {
        let skill_dir = home.join(".pi/agent/skills/squeez");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: squeez\ndescription: custom\n---\nuser content\n",
        )
        .unwrap();
        let a = PiAdapter;
        a.inject_memory(&Config::default(), &[]).unwrap();
        let body = std::fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
        assert!(body.contains("name: squeez"), "frontmatter not preserved");
        assert!(body.contains("description: custom"), "frontmatter not preserved");
        assert!(body.contains("<!-- squeez:start -->"));
        assert!(body.contains("user content"), "user content dropped");
    });
}
