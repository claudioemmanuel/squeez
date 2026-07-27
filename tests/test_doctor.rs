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
fn focus_puts_failures_first_and_ends_with_one_action() {
    let dir = tmp();
    let settings = healthy_install(&dir);
    let cfg = Config::from_str("enabled = false\nfocus = adhd\n");
    let (lines, has_fail) = doctor::run_with(&dir, &settings, &cfg);
    assert!(has_fail);
    // Line 0 is the version header; the first check line must be the failure.
    assert!(lines[1].starts_with("[FAIL]"), "got: {lines:?}");
    let last = lines.last().unwrap();
    assert!(last.starts_with("Next: "), "got: {last}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn focus_off_keeps_original_doctor_order() {
    let dir = tmp();
    let settings = healthy_install(&dir);
    let cfg = Config::from_str("enabled = false\n");
    let (lines, _) = doctor::run_with(&dir, &settings, &cfg);
    assert!(lines[1].starts_with("[ok]"), "got: {lines:?}");
    assert!(!lines.iter().any(|l| l.starts_with("Next: ")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn healthy_install_under_focus_says_nothing_to_fix() {
    let dir = tmp();
    let settings = healthy_install(&dir);
    let cfg = Config::from_str("focus = adhd\n");
    let (lines, _) = doctor::run_with(&dir, &settings, &cfg);
    assert_eq!(lines.last().unwrap(), "Next: nothing to fix — pipeline healthy.");
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

/// Bash output is compressed at PreToolUse, so its PostToolUse `track` record
/// is always `tokens_est:0`. A session of purely wrapped Bash calls still has
/// live accounting via the `type:"bash"` wrap record and must not warn.
#[test]
fn wrapped_bash_records_count_as_live_accounting() {
    let dir = tmp();
    let settings = healthy_install(&dir);
    std::fs::write(
        dir.join("sessions").join("2026-07-20-13.jsonl"),
        "{\"type\":\"bash\",\"cmd\":\"ls\",\"in_tk\":98,\"out_tk\":98,\"ts\":1}\n\
         {\"type\":\"tool\",\"tool\":\"Bash\",\"tokens_est\":0,\"ts\":2}\n",
    )
    .unwrap();
    let (lines, has_fail) = doctor::run_with(&dir, &settings, &Config::default());
    assert!(!has_fail, "got: {lines:?}");
    assert!(
        lines.iter().any(|l| l.starts_with("[ok]") && l.contains("freshness")),
        "wrapped Bash accounting must read as alive: {lines:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A wrap record whose in_tk/out_tk are both zero is not proof of life —
/// the genuine "pipeline dead" warning must survive.
#[test]
fn zeroed_bash_wrap_record_still_warns() {
    let dir = tmp();
    let settings = healthy_install(&dir);
    std::fs::write(
        dir.join("sessions").join("2026-07-20-13.jsonl"),
        "{\"type\":\"bash\",\"cmd\":\"ls\",\"in_tk\":0,\"out_tk\":0,\"ts\":1}\n\
         {\"type\":\"tool\",\"tool\":\"Bash\",\"tokens_est\":0,\"ts\":2}\n",
    )
    .unwrap();
    let (lines, _) = doctor::run_with(&dir, &settings, &Config::default());
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
