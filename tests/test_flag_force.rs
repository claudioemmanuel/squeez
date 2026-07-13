//! Integration tests for E3 flag forcing, spawning the real binary (same
//! pattern as tests/test_wrap_integration.rs and test_success_collapse.rs)
//! so the actual spawn_and_capture refactor and escape/fallback re-run are
//! exercised end-to-end, not just the pure `commands::flag_force` unit logic.
//!
//! Tier "full" tests use a throwaway shell shim named `jest` on PATH instead
//! of a real jest install, so these tests don't depend on Node/npm being
//! present and can precisely control whether the injected `--json` flag is
//! accepted or rejected.

use std::process::Command;

fn bin() -> String {
    env!("CARGO_BIN_EXE_squeez").to_string()
}

fn tmp_squeez_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "squeez_flag_force_it_{}_{}",
        label,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join("sessions")).unwrap();
    std::fs::create_dir_all(dir.join("memory")).unwrap();
    dir
}

/// A directory containing an executable shell shim named `jest`, meant to be
/// prepended to PATH. `body` is the shim's shell script body.
fn shim_dir(label: &str, body: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "squeez_flag_force_shim_{}_{}",
        label,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let shim = dir.join("jest");
    std::fs::write(&shim, format!("#!/bin/sh\n{}\n", body)).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&shim).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&shim, perms).unwrap();
    }
    dir
}

fn path_with_shim(shim: &std::path::Path) -> String {
    format!("{}:{}", shim.display(), std::env::var("PATH").unwrap_or_default())
}

#[test]
fn tier_env_sets_safe_formatting_vars() {
    let dir = tmp_squeez_dir("env_on");
    std::fs::write(dir.join("config.ini"), "net_win_min_tokens = 0\n").unwrap();

    // Clear these from the test process's own ambient environment first --
    // some dev/agent shells (this one included) already export NO_COLOR=1/
    // FORCE_COLOR=0, which would let the child inherit the "right" value
    // even if squeez's own env-tier logic were broken, masking a real
    // regression as a false-positive pass.
    let out = Command::new(bin())
        .args(["wrap", "echo \"NO_COLOR=[$NO_COLOR] FORCE_COLOR=[$FORCE_COLOR] LC_ALL=[$LC_ALL] GIT_PAGER=[$GIT_PAGER]\""])
        .env("SQUEEZ_DIR", &dir)
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR")
        .env_remove("LC_ALL")
        .env_remove("GIT_PAGER")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("NO_COLOR=[1]"), "got: {}", stdout);
    assert!(stdout.contains("FORCE_COLOR=[0]"), "got: {}", stdout);
    assert!(stdout.contains("LC_ALL=[C]"), "got: {}", stdout);
    assert!(stdout.contains("GIT_PAGER=[cat]"), "got: {}", stdout);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn tier_off_sets_no_env_vars() {
    let dir = tmp_squeez_dir("off");
    std::fs::write(dir.join("config.ini"), "net_win_min_tokens = 0\nflag_force = off\n").unwrap();

    // Explicitly clear these from the test process's own ambient
    // environment before spawning -- some dev/agent shells (this one
    // included) already export NO_COLOR=1/FORCE_COLOR=0 for their own
    // reasons, which the child would otherwise inherit regardless of
    // squeez's tier=off logic, making the assertion below environment-
    // dependent instead of actually testing what squeez does.
    let out = Command::new(bin())
        .args(["wrap", "echo \"NO_COLOR=[$NO_COLOR]\""])
        .env("SQUEEZ_DIR", &dir)
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("NO_COLOR=[]"), "got: {}", stdout);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn tier_full_injects_json_and_shows_forced_tag() {
    let dir = tmp_squeez_dir("full_ok");
    std::fs::write(dir.join("config.ini"), "net_win_min_tokens = 0\nflag_force = full\n").unwrap();
    let shim = shim_dir(
        "full_ok",
        r#"if [ "$1" = "--json" ]; then echo '{"numTotalTests":1,"numFailedTests":0,"numPassedTests":1,"numPendingTests":0,"numFailedTestSuites":0,"numTotalTestSuites":1,"success":true,"testResults":[]}'; exit 0; fi
echo "plain text output"; exit 0"#,
    );

    let out = Command::new(bin())
        .args(["wrap", "jest"])
        .env("SQUEEZ_DIR", &dir)
        .env("PATH", path_with_shim(&shim))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout.contains("[forced: --json]"), "got: {}", stdout);
    // The condensed jest_json reporter output, not the raw plain-text branch.
    assert!(stdout.contains("ok 1 passed"), "got: {}", stdout);

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&shim).ok();
}

#[test]
fn tier_full_falls_back_when_flag_rejected() {
    let dir = tmp_squeez_dir("full_reject");
    std::fs::write(dir.join("config.ini"), "net_win_min_tokens = 0\nflag_force = full\n").unwrap();
    let shim = shim_dir(
        "full_reject",
        r#"if [ "$1" = "--json" ]; then echo "error: unrecognized arguments: --json" >&2; exit 1; fi
echo "plain jest output, 5 passed"; exit 0"#,
    );

    let out = Command::new(bin())
        .args(["wrap", "jest"])
        .env("SQUEEZ_DIR", &dir)
        .env("PATH", path_with_shim(&shim))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    // The fallback re-run of the ORIGINAL command must succeed and its
    // output must be what's shown -- not the rejected --json attempt.
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout.contains("plain jest output"), "got: {}", stdout);
    assert!(!stdout.contains("[forced:"), "got: {}", stdout);
    assert!(!stdout.contains("unrecognized arguments"), "got: {}", stdout);

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&shim).ok();
}

#[test]
fn tier_full_never_injects_across_pipes() {
    let dir = tmp_squeez_dir("full_pipe");
    std::fs::write(dir.join("config.ini"), "net_win_min_tokens = 0\nflag_force = full\n").unwrap();
    let shim = shim_dir(
        "full_pipe",
        r#"if [ "$1" = "--json" ]; then echo "SHOULD NOT SEE THIS"; exit 0; fi
echo "line one"; echo "line two"; exit 0"#,
    );

    let out = Command::new(bin())
        .args(["wrap", "jest | grep line"])
        .env("SQUEEZ_DIR", &dir)
        .env("PATH", path_with_shim(&shim))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("SHOULD NOT SEE THIS"), "got: {}", stdout);
    assert!(!stdout.contains("[forced:"), "got: {}", stdout);

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&shim).ok();
}

#[test]
fn tier_full_deny_list_blocks_injection() {
    let dir = tmp_squeez_dir("full_deny");
    std::fs::write(
        dir.join("config.ini"),
        "net_win_min_tokens = 0\nflag_force = full\nflag_force_deny = jest\n",
    )
    .unwrap();
    let shim = shim_dir(
        "full_deny",
        r#"if [ "$1" = "--json" ]; then echo "SHOULD NOT SEE THIS"; exit 0; fi
echo "plain output"; exit 0"#,
    );

    let out = Command::new(bin())
        .args(["wrap", "jest"])
        .env("SQUEEZ_DIR", &dir)
        .env("PATH", path_with_shim(&shim))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("SHOULD NOT SEE THIS"), "got: {}", stdout);
    assert!(!stdout.contains("[forced:"), "got: {}", stdout);
    assert!(stdout.contains("plain output"), "got: {}", stdout);

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&shim).ok();
}
