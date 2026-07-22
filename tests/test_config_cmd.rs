//! End-to-end tests for the `squeez config` CLI, driving the real binary.
//! SQUEEZ_DIR is set per-subprocess (not on the test process) so parallel
//! tests never race on a shared env var.

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_squeez");

fn tmp_dir(tag: &str) -> PathBuf {
    let uniq = format!(
        "{}-{}-{}",
        tag,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        std::process::id()
    );
    let p = std::env::temp_dir().join(format!("squeez-config-{uniq}"));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn run(dir: &PathBuf, args: &[&str]) -> std::process::Output {
    Command::new(BIN)
        .arg("config")
        .args(args)
        .env("SQUEEZ_DIR", dir)
        .output()
        .expect("failed to run squeez")
}

#[test]
fn set_then_get_round_trips() {
    let dir = tmp_dir("rt");
    let out = run(&dir, &["set", "persona", "lite"]);
    assert!(out.status.success(), "set failed");

    let out = run(&dir, &["get", "persona"]);
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "lite");
}

#[test]
fn focus_normalizes_and_rejects_junk() {
    let dir = tmp_dir("focus");
    assert!(run(&dir, &["set", "focus", "ADHD"]).status.success());
    let out = run(&dir, &["get", "focus"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "adhd");

    let out = run(&dir, &["set", "focus", "focused"]);
    assert!(!out.status.success(), "unknown focus value must exit non-zero");
    assert!(String::from_utf8_lossy(&out.stderr).contains("off|adhd"));

    assert!(run(&dir, &["set", "focus", "off"]).status.success());
    let out = run(&dir, &["get", "focus"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "off");
}

#[test]
fn invalid_key_exits_non_zero() {
    let dir = tmp_dir("badkey");
    let out = run(&dir, &["set", "no_such_key", "1"]);
    assert!(!out.status.success(), "unknown key must exit non-zero");
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown key"));
}

#[test]
fn invalid_value_exits_non_zero() {
    let dir = tmp_dir("badval");
    let out = run(&dir, &["set", "max_lines", "lots"]);
    assert!(!out.status.success(), "type-invalid value must exit non-zero");
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid value"));
}

#[test]
fn reset_restores_default() {
    let dir = tmp_dir("reset");
    run(&dir, &["set", "find_max_results", "80"]);
    assert_eq!(
        String::from_utf8_lossy(&run(&dir, &["get", "find_max_results"]).stdout).trim(),
        "80"
    );
    run(&dir, &["reset", "find_max_results"]);
    assert_eq!(
        String::from_utf8_lossy(&run(&dir, &["get", "find_max_results"]).stdout).trim(),
        "50",
        "reset should restore the default"
    );
}

#[test]
fn list_json_is_wellformed_and_marks_changes() {
    let dir = tmp_dir("json");
    run(&dir, &["set", "max_lines", "200"]);
    let out = run(&dir, &["list", "--json"]);
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.trim_start().starts_with('['), "json must be an array");
    assert!(s.contains("\"key\":\"max_lines\""));
    assert!(s.contains("\"value\":\"200\""));
    assert!(s.contains("\"changed\":true"));
}

#[test]
fn path_prints_config_location() {
    let dir = tmp_dir("path");
    let out = run(&dir, &["path"]);
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.trim().ends_with("config.ini"), "got: {s}");
}
