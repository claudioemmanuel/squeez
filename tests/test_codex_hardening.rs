use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use squeez::hosts::{CodexCliAdapter, HostAdapter};

static ENV_GUARD: Mutex<()> = Mutex::new(());

fn python3_missing() -> bool {
    Command::new("python3")
        .arg("--version")
        .status()
        .map(|s| !s.success())
        .unwrap_or(true)
}

fn tmp_home() -> PathBuf {
    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let path = std::env::temp_dir().join(format!("squeez-codex-hardening-{unique}"));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn with_home<F: FnOnce(&Path)>(f: F) {
    let _guard = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let home = tmp_home();
    let old_home = std::env::var_os("HOME");
    let old_userprofile = std::env::var_os("USERPROFILE");
    std::env::set_var("HOME", &home);
    std::env::remove_var("USERPROFILE");
    f(&home);
    match old_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    match old_userprofile {
        Some(value) => std::env::set_var("USERPROFILE", value),
        None => std::env::remove_var("USERPROFILE"),
    }
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn codex_install_and_uninstall_preserve_unrelated_squeez_named_hook() {
    if python3_missing() {
        return;
    }
    with_home(|home| {
        let codex_dir = home.join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        let unrelated = "/tmp/my-squeez-hook.sh";
        std::fs::write(
            codex_dir.join("hooks.json"),
            format!(
                r#"{{"hooks":{{"PreToolUse":[{{"matcher":"special","hooks":[{{"type":"command","command":"{unrelated}"}}]}}]}}}}"#
            ),
        )
        .unwrap();

        let adapter = CodexCliAdapter;
        adapter.install(Path::new("/usr/local/bin/squeez")).unwrap();
        let installed = std::fs::read_to_string(codex_dir.join("hooks.json")).unwrap();
        assert!(installed.contains(unrelated));
        assert!(installed.contains("codex-pretooluse.sh"));

        adapter.uninstall().unwrap();
        let uninstalled = std::fs::read_to_string(codex_dir.join("hooks.json")).unwrap();
        assert!(uninstalled.contains(unrelated));
        assert!(!uninstalled.contains("codex-pretooluse.sh"));
    });
}

#[test]
fn codex_no_squeez_prefix_without_boundary_is_not_stripped() {
    if python3_missing() {
        return;
    }
    with_home(|home| {
        let bin_dir = home.join(".claude/squeez/bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let binary = bin_dir.join("squeez");
        std::fs::write(
            &binary,
            "#!/bin/sh\n[ \"$1\" = should-wrap ] && exit 1\nexit 0\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let hook = Path::new(env!("CARGO_MANIFEST_DIR")).join("hooks/codex-pretooluse.sh");
        let mut child = Command::new("bash")
            .arg(hook)
            .env("HOME", home)
            .env_remove("USERPROFILE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(
                br#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"--no-squeezfoo"}}"#,
            )
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert!(
            output.stdout.is_empty(),
            "invalid marker prefix must not rewrite the command: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    });
}
