//! Vendored pato-buddy: materialization, hook registration, statusLine
//! protection, buddy=off stripping, uninstall.

use std::path::PathBuf;
use std::sync::Mutex;

use squeez::hosts::{buddy, ClaudeCodeAdapter, HostAdapter};

// HOME is process-global; serialise tests that mutate it.
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
    let path = std::env::temp_dir().join(format!("squeez-buddy-test-{uniq}"));
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
    std::env::remove_var("SQUEEZ_DIR");
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

fn python3_available() -> bool {
    std::process::Command::new("python3")
        .arg("--version")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn settings_str(home: &PathBuf) -> String {
    std::fs::read_to_string(home.join(".claude/settings.json")).unwrap_or_default()
}

fn write_config(home: &PathBuf, body: &str) {
    let dir = home.join(".claude/squeez");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.ini"), body).unwrap();
}

#[test]
fn materialize_writes_tree_version_and_exec_shims() {
    with_home(|home| {
        let data = home.join(".claude/squeez");
        let root = buddy::materialize(&data).expect("materialize");
        for rel in [
            "lib/state.js",
            "lib/engine.js",
            "lib/art.js",
            "lib/squeez.js",
            "hooks/session-start.js",
            "hooks/post-tool-use.js",
            "hooks/stop.js",
            "statusline/statusline.js",
            "shims/session-start.sh",
            "shims/post-tool-use.sh",
            "shims/stop.sh",
            "shims/statusline.sh",
            "VERSION",
        ] {
            assert!(root.join(rel).exists(), "missing {rel}");
        }
        assert_eq!(
            std::fs::read_to_string(root.join("VERSION")).unwrap(),
            env!("CARGO_PKG_VERSION")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(root.join("shims/stop.sh"))
                .unwrap()
                .permissions()
                .mode();
            assert!(mode & 0o111 != 0, "shim not executable");
        }
        // Shims must exec their own target, not each other's.
        let shim = std::fs::read_to_string(root.join("shims/statusline.sh")).unwrap();
        assert!(shim.contains("statusline/statusline.js"));
        assert!(shim.contains("buddy[[:space:]]*=[[:space:]]*(false|off|0)"));
    });
}

#[test]
fn install_registers_buddy_hooks_and_statusline_when_absent() {
    if !python3_available() {
        return;
    }
    with_home(|home| {
        ClaudeCodeAdapter.install(&home.join("bin/squeez")).unwrap();
        let s = settings_str(home);
        assert!(s.contains("buddy/shims/session-start.sh"), "SessionStart shim");
        assert!(s.contains("buddy/shims/post-tool-use.sh"), "PostToolUse shim");
        assert!(s.contains("buddy/shims/stop.sh"), "Stop shim");
        assert!(s.contains("Edit|Write|Bash"), "buddy PostToolUse matcher");
        assert!(s.contains("\"Stop\""), "Stop event registered");
        assert!(s.contains("buddy/shims/statusline.sh"), "statusline claimed");
        // Materialized tree exists.
        assert!(home.join(".claude/squeez/buddy/lib/squeez.js").exists());
    });
}

#[test]
fn install_never_clobbers_existing_statusline() {
    if !python3_available() {
        return;
    }
    with_home(|home| {
        let dir = home.join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"statusLine":{"type":"command","command":"claude-hud"}}"#,
        )
        .unwrap();
        ClaudeCodeAdapter.install(&home.join("bin/squeez")).unwrap();
        let s = settings_str(home);
        assert!(s.contains("claude-hud"), "existing statusLine survives");
        assert!(!s.contains("buddy/shims/statusline.sh"), "buddy must not claim it");
        // Hooks still registered.
        assert!(s.contains("buddy/shims/stop.sh"));
    });
}

#[test]
fn install_is_idempotent_and_core_hooks_survive_buddy_presence() {
    if !python3_available() {
        return;
    }
    with_home(|home| {
        ClaudeCodeAdapter.install(&home.join("bin/squeez")).unwrap();
        ClaudeCodeAdapter.install(&home.join("bin/squeez")).unwrap();
        let s = settings_str(home);
        // One buddy entry per event, not two.
        assert_eq!(s.matches("buddy/shims/stop.sh").count(), 1, "Stop duplicated");
        assert_eq!(
            s.matches("buddy/shims/session-start.sh").count(),
            1,
            "SessionStart duplicated"
        );
        // has_squeez regression: core hooks must still be present alongside
        // buddy entries (buddy paths contain "squeez" but must not satisfy
        // the core idempotency check).
        assert_eq!(s.matches("session-start.sh").count(), 2, "core + buddy session-start");
        assert!(s.contains("hooks/posttooluse.sh"), "core posttooluse");
    });
}

#[test]
fn buddy_off_strips_entries_and_statusline() {
    if !python3_available() {
        return;
    }
    with_home(|home| {
        // Default install: buddy on.
        ClaudeCodeAdapter.install(&home.join("bin/squeez")).unwrap();
        assert!(settings_str(home).contains("buddy/shims/stop.sh"));
        // Disable and re-run setup.
        write_config(home, "buddy = false\n");
        ClaudeCodeAdapter.install(&home.join("bin/squeez")).unwrap();
        let s = settings_str(home);
        assert!(!s.contains("/buddy/"), "buddy entries stripped: {s}");
        // Core hooks untouched.
        assert!(s.contains("hooks/posttooluse.sh"));
    });
}

#[test]
fn uninstall_removes_buddy_registration() {
    if !python3_available() {
        return;
    }
    with_home(|home| {
        ClaudeCodeAdapter.install(&home.join("bin/squeez")).unwrap();
        ClaudeCodeAdapter.uninstall().unwrap();
        let s = settings_str(home);
        assert!(!s.contains("/buddy/"), "buddy gone after uninstall: {s}");
        assert!(!s.contains("\"Stop\""), "Stop event cleaned");
    });
}
