//! `squeez update` must refresh hooks with the binary it just installed, not
//! with the outgoing process's embedded scripts (#237).

use std::path::{Path, PathBuf};

use squeez::commands::update::refresh_hooks;

fn tmp(label: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "squeez_update_refresh_{label}_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[cfg(unix)]
fn fake_binary(dir: &Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let bin = dir.join("squeez");
    std::fs::write(&bin, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

/// The refresh is delegated to the installed binary — whatever it is — with
/// exactly the setup invocation, and its report is relayed.
#[cfg(unix)]
#[test]
fn refresh_runs_the_installed_binary() {
    let dir = tmp("fake");
    let args = dir.join("args");
    let bin = fake_binary(
        &dir,
        &format!("echo \"$@\" > '{}'; echo 'squeez setup: claude-code  ✓ installed'", args.display()),
    );
    let out = refresh_hooks(&bin).expect("refresh must succeed");
    assert_eq!(std::fs::read_to_string(&args).unwrap().trim(), "setup --host=claude-code");
    assert!(out.contains("claude-code  ✓ installed"), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_failed_refresh_is_reported_not_swallowed() {
    let dir = tmp("fail");
    let bin = fake_binary(&dir, "echo boom >&2; exit 3");
    let err = refresh_hooks(&bin).expect_err("nonzero exit must be an error");
    assert!(err.contains("exited 3") && err.contains("boom"), "{err}");
    assert!(refresh_hooks(&dir.join("missing")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// End to end with the real binary: a stale installed hook is replaced by the
/// script this build embeds.
#[test]
fn refresh_replaces_stale_hook_scripts() {
    let home = tmp("home");
    let hooks = home.join(".claude").join("squeez").join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    std::fs::write(hooks.join("session-start.sh"), "# stale script from the old binary\n").unwrap();
    std::env::set_var("HOME", &home);
    std::env::remove_var("SQUEEZ_DIR");

    refresh_hooks(Path::new(env!("CARGO_BIN_EXE_squeez"))).expect("refresh must succeed");

    assert_eq!(
        std::fs::read_to_string(hooks.join("session-start.sh")).unwrap(),
        include_str!("../hooks/session-start.sh")
    );
    let settings = std::fs::read_to_string(home.join(".claude").join("settings.json")).unwrap();
    assert!(settings.contains("session-start.sh"), "{settings}");
    let _ = std::fs::remove_dir_all(&home);
}
