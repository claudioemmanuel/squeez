//! Integration tests for E5 success collapse, spawning the real binary
//! (matches the tests/test_wrap_integration.rs pattern) so exit-code
//! propagation from the actual subprocess is exercised for real, not just
//! the pure `strategies::success_collapse` unit logic.
//!
//! Runs `git add --dry-run ...` (prefix-eligible, never actually stages
//! anything) against a throwaway git repo per test -- both to avoid
//! `.git/index.lock` contention with other tests/processes running against
//! this crate's own repo in parallel, and because it's more hermetic than
//! depending on live repo state anyway. Real git/npm/docker network
//! operations stay covered by the unit tests in strategies/success_collapse.rs.

use std::process::Command;

fn bin() -> String {
    env!("CARGO_BIN_EXE_squeez").to_string()
}

fn tmp_squeez_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "squeez_success_collapse_it_{}_{}",
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

/// A throwaway git repo with one untracked file, isolated from this crate's
/// own repo so concurrent test runs never race on `.git/index.lock`.
fn tmp_git_repo(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "squeez_success_collapse_repo_{}_{}",
        label,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let init = Command::new("git").args(["init", "-q"]).current_dir(&dir).output().unwrap();
    assert!(init.status.success(), "git init failed: {:?}", init);
    std::fs::write(dir.join("scratch.txt"), "hello\n").unwrap();
    dir
}

#[test]
fn eligible_success_collapses_to_ok_line() {
    let dir = tmp_squeez_dir("collapse");
    std::fs::write(dir.join("config.ini"), "net_win_min_tokens = 0\n").unwrap();
    let repo = tmp_git_repo("collapse");

    let out = Command::new(bin())
        .args(["wrap", "git add --dry-run --all"])
        .current_dir(&repo)
        .env("SQUEEZ_DIR", &dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout.contains("ok git add --dry-run --all"), "got: {}", stdout);
    // Per-file "add 'path'" lines must not appear verbatim once collapsed.
    assert!(!stdout.contains("add '"), "expected file list collapsed, got: {}", stdout);
    // Original must still be recoverable via the retrieve stash even though
    // the raw dry-run output is well under the usual retrieve_min_lines gate.
    assert!(stdout.contains("squeez_retrieve"), "expected a retrieve marker, got: {}", stdout);

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&repo).ok();
}

#[test]
fn config_disable_restores_full_output() {
    let dir = tmp_squeez_dir("disabled");
    std::fs::write(dir.join("config.ini"), "net_win_min_tokens = 0\nsuccess_collapse = false\n").unwrap();
    let repo = tmp_git_repo("disabled");

    let out = Command::new(bin())
        .args(["wrap", "git add --dry-run --all"])
        .current_dir(&repo)
        .env("SQUEEZ_DIR", &dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0));
    assert!(!stdout.contains("ok git add --dry-run --all"), "got: {}", stdout);
    assert!(stdout.contains("add '"), "expected uncollapsed file list, got: {}", stdout);

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&repo).ok();
}

#[test]
fn deny_list_excludes_matching_prefix() {
    let dir = tmp_squeez_dir("deny");
    std::fs::write(
        dir.join("config.ini"),
        "net_win_min_tokens = 0\nsuccess_collapse_deny = git add\n",
    )
    .unwrap();
    let repo = tmp_git_repo("deny");

    let out = Command::new(bin())
        .args(["wrap", "git add --dry-run --all"])
        .current_dir(&repo)
        .env("SQUEEZ_DIR", &dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0));
    assert!(!stdout.contains("ok git add --dry-run --all"), "got: {}", stdout);
    assert!(stdout.contains("add '"), "expected uncollapsed file list, got: {}", stdout);

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&repo).ok();
}

#[test]
fn nonzero_exit_never_collapses() {
    let dir = tmp_squeez_dir("nonzero");
    std::fs::write(dir.join("config.ini"), "net_win_min_tokens = 0\n").unwrap();
    let repo = tmp_git_repo("nonzero");

    // A pathspec that matches nothing makes git add fail with a fatal error
    // and a non-zero exit -- still prefix-eligible, must not collapse.
    let out = Command::new(bin())
        .args(["wrap", "git add --dry-run nonexistent-file-squeez-test-xyz.txt"])
        .current_dir(&repo)
        .env("SQUEEZ_DIR", &dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_ne!(out.status.code(), Some(0));
    assert!(!stdout.contains("ok git add"), "got: {}", stdout);

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&repo).ok();
}
