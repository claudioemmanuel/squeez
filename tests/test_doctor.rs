use squeez::commands::doctor;
use squeez::config::Config;
use squeez::hosts::claude_code::hooks_manifest;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn tmp() -> PathBuf {
    static CTR: AtomicU64 = AtomicU64::new(0);
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "squeez_doctor_test_{}_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        n
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Healthy install: embedded hooks on disk, hooks referenced in settings,
/// fresh handler_stats + a session log with nonzero tokens_est.
fn healthy_install(dir: &PathBuf) -> PathBuf {
    let hooks = dir.join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    for (name, content) in hooks_manifest() {
        std::fs::write(hooks.join(name), content).unwrap();
    }
    let sessions = dir.join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(sessions.join("handler_stats.json"), "{}").unwrap();
    std::fs::write(
        sessions.join("2026-07-20-13.jsonl"),
        "{\"type\":\"tool\",\"tool\":\"Bash\",\"tokens_est\":42,\"ts\":1}\n",
    )
    .unwrap();
    let settings = dir.join("settings.json");
    std::fs::write(
        &settings,
        r#"{"hooks":{"PreToolUse":[{"hooks":[{"command":"bash /x/pretooluse.sh"}]}],"PostToolUse":[{"hooks":[{"command":"bash /x/posttooluse.sh"}]}],"SessionStart":[{"hooks":[{"command":"bash /x/session-start.sh"}]}]}}"#,
    )
    .unwrap();
    settings
}

#[test]
fn healthy_install_passes_all_checks() {
    let dir = tmp();
    let settings = healthy_install(&dir);
    let (lines, has_fail) = doctor::run_with(&dir, &settings, &Config::default());
    assert!(!has_fail, "expected no FAIL, got: {lines:?}");
    assert!(lines.iter().all(|l| !l.starts_with("[FAIL]")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn disabled_config_is_a_fail() {
    let dir = tmp();
    let settings = healthy_install(&dir);
    let cfg = Config::from_str("enabled = false\n");
    let (lines, has_fail) = doctor::run_with(&dir, &settings, &cfg);
    assert!(has_fail);
    assert!(
        lines.iter().any(|l| l.starts_with("[FAIL]") && l.contains("enabled=false")),
        "got: {lines:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tampered_hook_is_stale() {
    let dir = tmp();
    let settings = healthy_install(&dir);
    std::fs::write(dir.join("hooks").join("posttooluse.sh"), "#!/bin/bash\n# old\n").unwrap();
    let (lines, has_fail) = doctor::run_with(&dir, &settings, &Config::default());
    assert!(has_fail);
    assert!(
        lines
            .iter()
            .any(|l| l.contains("posttooluse.sh") && l.contains("squeez setup")),
        "got: {lines:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_settings_is_a_fail() {
    let dir = tmp();
    healthy_install(&dir);
    let (lines, has_fail) =
        doctor::run_with(&dir, &dir.join("nope.json"), &Config::default());
    assert!(has_fail, "got: {lines:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn zeroed_tokens_est_warns_tracking_dead() {
    let dir = tmp();
    let settings = healthy_install(&dir);
    std::fs::write(
        dir.join("sessions").join("2026-07-20-13.jsonl"),
        "{\"type\":\"tool\",\"tool\":\"Bash\",\"tokens_est\":0,\"ts\":1}\n",
    )
    .unwrap();
    let (lines, has_fail) = doctor::run_with(&dir, &settings, &Config::default());
    assert!(!has_fail, "WARN must not be a FAIL: {lines:?}");
    assert!(
        lines.iter().any(|l| l.starts_with("[WARN]") && l.contains("tokens_est:0")),
        "got: {lines:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn quick_check_silent_when_healthy_and_loud_when_disabled() {
    let dir = tmp();
    let settings = healthy_install(&dir);
    assert_eq!(doctor::quick_check(&dir, &settings, &Config::default()), None);
    let cfg = Config::from_str("enabled = false\n");
    let w = doctor::quick_check(&dir, &settings, &cfg).expect("warning line");
    assert!(w.contains("squeez doctor"));
    assert!(w.len() <= 80, "banner line must stay short: {}", w.len());
    let _ = std::fs::remove_dir_all(&dir);
}
