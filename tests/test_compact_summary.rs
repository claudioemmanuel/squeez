//! `squeez compact-summary --session-start` — post-compaction state restore
//! delivered through SessionStart (#225).

use std::io::Write;
use std::process::{Command, Stdio};

use squeez::context::cache::{FileAccess, SessionContext};

fn tmp() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "squeez_compact_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(d.join("sessions")).unwrap();
    d
}

fn run_session_start(dir: &std::path::Path, payload: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_squeez"))
        .args(["compact-summary", "--session-start"])
        .env("SQUEEZ_DIR", dir)
        .env("HOME", dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn squeez");
    child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "exited nonzero: {out:?}");
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn seeded() -> std::path::PathBuf {
    let dir = tmp();
    let mut ctx = SessionContext::default();
    ctx.note_file(r"C:\Users\You\src\main.rs", FileAccess::Read);
    ctx.save(&dir.join("sessions"));
    dir
}

#[test]
fn compact_start_restores_state_as_plain_text() {
    let dir = seeded();
    let out = run_session_start(&dir, r#"{"session_id":"s1","source":"compact"}"#);
    assert!(out.starts_with("[squeez session state"), "got: {out:?}");
    // Plain text, never hook JSON: PostCompact's hookSpecificOutput shape is
    // rejected by Claude Code, and SessionStart stdout is already context.
    assert!(!out.contains("hookSpecificOutput"), "got: {out:?}");
    // Decoded once — no doubled backslashes (#229).
    assert!(out.contains(r"C:\Users\You\src\main.rs(R)"), "got: {out:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn other_session_starts_print_nothing() {
    let dir = seeded();
    for src in ["startup", "resume", "clear"] {
        let out = run_session_start(&dir, &format!(r#"{{"source":"{src}"}}"#));
        assert!(out.is_empty(), "source={src} printed: {out:?}");
    }
    assert!(run_session_start(&dir, "").is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn postcompact_hook_no_longer_emits_the_summary() {
    let script = include_str!("../hooks/postcompact.sh");
    let code: Vec<&str> = script.lines().filter(|l| !l.trim_start().starts_with('#')).collect();
    assert!(!code.iter().any(|l| l.contains("compact-summary")), "{script}");
    let start = include_str!("../hooks/session-start.sh");
    assert!(start.contains("compact-summary --session-start"));
}
