//! Integration test for E7 tier 2 (path denylist), spawning the real
//! binary (same pattern as test_success_collapse.rs / test_flag_force.rs)
//! so the wrap.rs wiring is exercised end-to-end, not just the pure
//! `context::sensitive::path_denylist_match` unit logic.
//!
//! The match is on the COMMAND STRING alone (no file needs to exist), so
//! these tests use a harmless `echo` that merely mentions the path --
//! hermetic, no real file I/O, safe under parallel test execution.

use std::process::Command;

fn bin() -> String {
    env!("CARGO_BIN_EXE_squeez").to_string()
}

fn tmp_squeez_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "squeez_sensitive_path_it_{}_{}",
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

#[test]
fn cmd_referencing_dotenv_gets_warned() {
    let dir = tmp_squeez_dir("dotenv");
    std::fs::write(dir.join("config.ini"), "net_win_min_tokens = 0\n").unwrap();

    let out = Command::new(bin())
        .args(["wrap", "echo checking .env now"])
        .env("SQUEEZ_DIR", &dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("sensitive-looking path") && stdout.contains("\".env\""),
        "got: {}",
        stdout
    );
    // Warn-only: the actual output must still print (never blocked).
    assert!(stdout.contains("checking .env now"), "got: {}", stdout);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cmd_referencing_ssh_key_path_gets_warned() {
    let dir = tmp_squeez_dir("sshkey");
    std::fs::write(dir.join("config.ini"), "net_win_min_tokens = 0\n").unwrap();

    let out = Command::new(bin())
        .args(["wrap", "echo about to read ~/.ssh/id_rsa"])
        .env("SQUEEZ_DIR", &dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("sensitive-looking path"), "got: {}", stdout);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn ordinary_command_gets_no_sensitive_warning() {
    let dir = tmp_squeez_dir("ordinary");
    std::fs::write(dir.join("config.ini"), "net_win_min_tokens = 0\n").unwrap();

    let out = Command::new(bin())
        .args(["wrap", "echo hello world"])
        .env("SQUEEZ_DIR", &dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("sensitive-looking path"), "got: {}", stdout);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn dotenv_example_still_warns_but_does_not_block_output() {
    // Deliberate per path_denylist_match's doc: this tier is warn-only, so
    // a command mentioning ".env.example" (a common non-secret filename)
    // still matches and still warns -- it must NOT suppress the output.
    let dir = tmp_squeez_dir("dotenv_example");
    std::fs::write(dir.join("config.ini"), "net_win_min_tokens = 0\n").unwrap();

    let out = Command::new(bin())
        .args(["wrap", "echo copying .env.example to .env"])
        .env("SQUEEZ_DIR", &dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("sensitive-looking path"), "got: {}", stdout);
    assert!(stdout.contains("copying .env.example to .env"), "got: {}", stdout);

    std::fs::remove_dir_all(&dir).ok();
}
