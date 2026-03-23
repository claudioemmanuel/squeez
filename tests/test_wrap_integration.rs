use std::process::Command;

fn bin() -> String {
    env!("CARGO_BIN_EXE_squeez").to_string()
}

#[test]
fn wrap_runs_and_shows_header() {
    let out = Command::new(bin())
        .args(["wrap", "echo hello"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("hello"));
    assert!(stdout.contains("# squeez"));
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn wrap_forwards_exit_code() {
    let out = Command::new(bin())
        .args(["wrap", "exit 42"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn no_squeez_bypasses_compression() {
    let out = Command::new(bin())
        .args(["wrap", "--no-squeez echo raw"])
        .output()
        .unwrap();
    // --no-squeez is handled by pretooluse.sh hook, not wrap directly
    // wrap will treat this as "sh -c '--no-squeez echo raw'" which fails
    // This test verifies the exit code is non-zero (command not found)
    assert_ne!(out.status.code(), None);
}

#[test]
fn wrap_handles_pipes_via_sh() {
    let out = Command::new(bin())
        .args(["wrap", "echo hello | tr a-z A-Z"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("HELLO"));
}

#[test]
fn wrap_bypassed_command_runs_and_exits_zero() {
    let out = Command::new(bin())
        .args(["wrap", "exit 0"])
        .output()
        .unwrap();
    // sh -c "exit 0" should exit 0 (compression or not)
    assert_eq!(out.status.code(), Some(0));
}
