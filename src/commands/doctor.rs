//! `squeez doctor` — read-only self-health check for the compression pipeline.
//!
//! Motivated by a real outage: `enabled=false` plus a payload-key drift left
//! the whole pipeline dead for days while the session banner claimed
//! "Compression: ON". Doctor makes that state visible: hook drift vs the
//! embedded sources, hook registration, config state, and tracking freshness.

use crate::config::Config;
use crate::hosts::claude_code::hooks_manifest;
use crate::session;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// One check line plus whether it is a hard failure.
struct CheckLine {
    line: String,
    fail: bool,
}

fn ok(msg: impl Into<String>) -> CheckLine {
    CheckLine { line: format!("[ok]   {}", msg.into()), fail: false }
}
fn warn(msg: impl Into<String>) -> CheckLine {
    CheckLine { line: format!("[WARN] {}", msg.into()), fail: false }
}
fn fail(msg: impl Into<String>) -> CheckLine {
    CheckLine { line: format!("[FAIL] {}", msg.into()), fail: true }
}

/// Hook-drift check: installed scripts must match the sources embedded in
/// this binary. A stale hook means the binary was updated but `squeez setup`
/// never re-ran (or someone edited the installed copy).
fn check_hooks_drift(squeez_dir: &Path) -> CheckLine {
    let mut stale: Vec<&str> = Vec::new();
    for (name, embedded) in hooks_manifest() {
        let installed = squeez_dir.join("hooks").join(name);
        match std::fs::read_to_string(&installed) {
            Ok(content) if content == embedded => {}
            _ => stale.push(name),
        }
    }
    if stale.is_empty() {
        ok("hooks: installed scripts match this binary")
    } else {
        fail(format!(
            "hooks: stale or missing ({}) — run `squeez setup`",
            stale.join(", ")
        ))
    }
}

/// Registration check: the host settings file must reference the squeez hooks.
fn check_hooks_registered(settings_path: &Path) -> CheckLine {
    let content = std::fs::read_to_string(settings_path).unwrap_or_default();
    if content.is_empty() {
        return fail(format!(
            "registration: {} missing or unreadable — run `squeez setup`",
            settings_path.display()
        ));
    }
    let missing: Vec<&str> = ["pretooluse.sh", "posttooluse.sh", "session-start.sh"]
        .into_iter()
        .filter(|h| !content.contains(h))
        .collect();
    if missing.is_empty() {
        ok("registration: hooks present in settings.json")
    } else {
        fail(format!(
            "registration: {} not registered — run `squeez setup`",
            missing.join(", ")
        ))
    }
}

/// Config check: a disabled pipeline is the exact silent-death mode doctor
/// exists to surface, so it is a FAIL, not a WARN.
fn check_config(cfg: &Config) -> CheckLine {
    let label = cfg.compression_status_label();
    if !cfg.enabled {
        fail(format!("config: compression {}", label))
    } else if !cfg.wrap_bash {
        warn(format!("config: compression {}", label))
    } else {
        ok("config: compression ON")
    }
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Newest session-log (`*.jsonl`) mtime in the sessions dir.
fn newest_session(sessions: &Path) -> Option<SystemTime> {
    let mut newest: Option<SystemTime> = None;
    for e in std::fs::read_dir(sessions).ok()?.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) == Some("jsonl") {
            if let Some(t) = mtime(&p) {
                newest = Some(newest.map_or(t, |n| n.max(t)));
            }
        }
    }
    newest
}

/// True if any of the newest session logs carries a nonzero tokens_est —
/// distinguishes "accounting alive" from the all-zeros drift outage.
fn tokens_est_alive(sessions: &Path) -> bool {
    let mut files: Vec<(SystemTime, std::path::PathBuf)> = std::fs::read_dir(sessions)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            (p.extension().and_then(|s| s.to_str()) == Some("jsonl"))
                .then(|| mtime(&p).map(|t| (t, p)))
                .flatten()
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.iter().take(2).any(|(_, p)| {
        std::fs::read_to_string(p).is_ok_and(|s| {
            s.lines().any(|l| {
                l.split("\"tokens_est\":")
                    .nth(1)
                    .and_then(|rest| {
                        rest.trim_start()
                            .split(|c: char| !c.is_ascii_digit())
                            .next()?
                            .parse::<u64>()
                            .ok()
                    })
                    .is_some_and(|v| v > 0)
            })
        })
    })
}

/// Freshness check: sessions are running but the wrap/accounting artifacts
/// stopped moving — the "tracking dead" signature of the July outage.
fn check_freshness(squeez_dir: &Path, cfg: &Config) -> CheckLine {
    if cfg.doctor_stale_days == 0 {
        return ok("freshness: check disabled (doctor_stale_days=0)");
    }
    let sessions = squeez_dir.join("sessions");
    let Some(newest) = newest_session(&sessions) else {
        return ok("freshness: no session logs yet");
    };
    let window = Duration::from_secs(cfg.doctor_stale_days * 24 * 3600);
    let stats_stale = mtime(&sessions.join("handler_stats.json"))
        .map_or(true, |t| newest > t + window);
    if stats_stale {
        return warn(
            "freshness: sessions active but handler_stats.json is stale — \
             bash wrap not running (check `squeez should-wrap`, config, hooks)",
        );
    }
    if !tokens_est_alive(&sessions) {
        return warn(
            "freshness: recent session logs have only tokens_est:0 — \
             accounting broken (posttooluse payload extraction)",
        );
    }
    ok("freshness: tracking artifacts are current")
}

/// Full doctor report. Returns the printable lines and whether any check FAILed.
pub fn run_with(squeez_dir: &Path, settings_path: &Path, cfg: &Config) -> (Vec<String>, bool) {
    let checks = [
        check_hooks_drift(squeez_dir),
        check_hooks_registered(settings_path),
        check_config(cfg),
        check_freshness(squeez_dir, cfg),
    ];
    let has_fail = checks.iter().any(|c| c.fail);
    let mut lines: Vec<String> = vec![format!(
        "squeez doctor — v{} @ {}",
        env!("CARGO_PKG_VERSION"),
        squeez_dir.display()
    )];
    lines.extend(checks.into_iter().map(|c| c.line));
    (lines, has_fail)
}

/// Cheap subset for the SessionStart banner (no session-log scan): returns a
/// single warning line when the pipeline is degraded, or None when healthy.
pub fn quick_check(squeez_dir: &Path, settings_path: &Path, cfg: &Config) -> Option<String> {
    let degraded = check_hooks_drift(squeez_dir).fail
        || check_hooks_registered(settings_path).fail
        || check_config(cfg).fail;
    degraded.then(|| "[squeez doctor: pipeline degraded — run 'squeez doctor']".to_string())
}

fn default_settings_path() -> std::path::PathBuf {
    Path::new(&session::home_dir()).join(".claude").join("settings.json")
}

/// CLI entry point: print the report, exit 1 on any FAIL.
pub fn run() -> i32 {
    let cfg = Config::load();
    let (lines, has_fail) = run_with(&session::squeez_dir(), &default_settings_path(), &cfg);
    for l in lines {
        println!("{}", l);
    }
    i32::from(has_fail)
}
