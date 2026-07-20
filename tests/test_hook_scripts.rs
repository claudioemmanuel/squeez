//! Shell↔Rust contract tests for the hook scripts.
//!
//! The tokens_est:0 outage shipped because hooks/posttooluse.sh read the
//! payload key `tool_result` while Claude Code sends `tool_response` — the
//! Rust fixtures knew the real shape but nothing exercised the shell side.
//! These tests run the actual scripts with a stub `squeez` binary and the
//! real payload shapes, so shell/Rust drift fails CI instead of shipping.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

fn tmp() -> PathBuf {
    static CTR: AtomicU64 = AtomicU64::new(0);
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "squeez_hook_test_{}_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        n
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn have(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

fn repo_hook(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("hooks").join(name)
}

/// Write a stub `squeez` that appends `argv` to calls.log and swallows stdin.
fn write_stub(dir: &Path) -> PathBuf {
    let stub = dir.join("squeez");
    let log = dir.join("calls.log");
    std::fs::write(
        &stub,
        format!("#!/bin/bash\necho \"$@\" >> \"{}\"\ncat > /dev/null\n", log.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    stub
}

/// Run a hook script with `payload` on stdin and the stub as SQUEEZ_BIN;
/// return the recorded stub calls (one per line).
fn run_hook(script: &str, payload: &str, dir: &Path) -> Vec<String> {
    let stub = write_stub(dir);
    let log = dir.join("calls.log");
    let _ = std::fs::remove_file(&log);
    let mut child = Command::new("bash")
        .arg(repo_hook(script))
        .env("SQUEEZ_BIN", &stub)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn hook script");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    let status = child.wait().unwrap();
    assert!(status.success(), "{script} exited nonzero");
    std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// Extract the size argument from the recorded `track <tool> <size>` call.
fn track_size(calls: &[String], tool: &str) -> Option<u64> {
    calls.iter().find_map(|c| {
        let mut parts = c.split_whitespace();
        (parts.next() == Some("track") && parts.next() == Some(tool))
            .then(|| parts.next().and_then(|s| s.parse().ok()))
            .flatten()
    })
}

fn guard() -> bool {
    if !have("bash") || !have("python3") {
        eprintln!("skipping: bash/python3 unavailable");
        return false;
    }
    true
}

// ── posttooluse.sh: real Claude Code payload shapes ──────────────────────────

#[test]
fn posttooluse_tool_response_string() {
    if !guard() {
        return;
    }
    let dir = tmp();
    let calls = run_hook(
        "posttooluse.sh",
        r#"{"tool_name":"Bash","tool_response":"plain string output 1234"}"#,
        &dir,
    );
    assert_eq!(track_size(&calls, "Bash"), Some(24));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn posttooluse_read_nested_file_content() {
    if !guard() {
        return;
    }
    let dir = tmp();
    // Real Claude Code Read shape (see compress_output.rs fixtures).
    let calls = run_hook(
        "posttooluse.sh",
        r#"{"tool_name":"Read","tool_input":{"file_path":"/tmp/a.rs"},"tool_response":{"type":"text","file":{"filePath":"/tmp/a.rs","content":"hello world content here","numLines":1,"startLine":1,"totalLines":1}}}"#,
        &dir,
    );
    assert_eq!(track_size(&calls, "Read"), Some(24));
    // Read must also flow through compress-output then track-result, in order.
    let names: Vec<&str> = calls
        .iter()
        .map(|c| c.split_whitespace().next().unwrap_or(""))
        .collect();
    assert_eq!(names, ["track", "compress-output", "track-result"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn posttooluse_content_block_list() {
    if !guard() {
        return;
    }
    let dir = tmp();
    let calls = run_hook(
        "posttooluse.sh",
        r#"{"tool_name":"Grep","tool_response":{"content":[{"type":"text","text":"block one"},{"type":"text","text":"block two"}]}}"#,
        &dir,
    );
    assert_eq!(track_size(&calls, "Grep"), Some(18));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn posttooluse_image_block_does_not_crash() {
    if !guard() {
        return;
    }
    let dir = tmp();
    let calls = run_hook(
        "posttooluse.sh",
        r#"{"tool_name":"mcp__claude-in-chrome__computer","tool_response":{"content":[{"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"/9j/4AAQSkZJRg=="}}]}}"#,
        &dir,
    );
    // Image blocks carry no text — size 0 is correct; the hook must not die
    // and the MCP result must still reach compress-output (image dedup path).
    assert_eq!(
        track_size(&calls, "mcp__claude-in-chrome__computer"),
        Some(0)
    );
    assert!(calls.iter().any(|c| c.starts_with("compress-output")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn posttooluse_legacy_tool_result_fallback() {
    if !guard() {
        return;
    }
    let dir = tmp();
    let calls = run_hook(
        "posttooluse.sh",
        r#"{"tool_name":"Read","tool_result":{"content":"legacy shape"}}"#,
        &dir,
    );
    assert_eq!(track_size(&calls, "Read"), Some(12));
    let _ = std::fs::remove_dir_all(&dir);
}

// ── subagent-stop.sh ─────────────────────────────────────────────────────────

#[test]
fn subagent_stop_wraps_last_assistant_message() {
    if !guard() {
        return;
    }
    let dir = tmp();
    let calls = run_hook(
        "subagent-stop.sh",
        r#"{"last_assistant_message":"done: 3 files reviewed","agent_id":"a1"}"#,
        &dir,
    );
    assert!(
        calls.iter().any(|c| c.starts_with("track-result SubagentStop")),
        "expected track-result SubagentStop, got {calls:?}"
    );
    assert!(calls.iter().any(|c| c.starts_with("track SubagentStop")));
    let _ = std::fs::remove_dir_all(&dir);
}
