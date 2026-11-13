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
        format!(
            "#!/bin/bash\necho \"$@\" >> \"{}\"\ncat >> \"{}\"\n",
            log.display(),
            dir.join("stdin.log").display()
        ),
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

// ── pretooluse.sh / posttooluse.sh: thin wrappers over `squeez hook` ─────────
//
// The payload-shape contract (tool_response vs tool_result, Read's nested
// file.content, content-block lists, images) now lives in Rust:
// `commands::hook` unit tests. Here: the scripts hand stdin to the binary
// verbatim, and the real binary behind the real script does the work.

const READ_PAYLOAD: &str = r#"{"tool_name":"Read","tool_input":{"file_path":"/tmp/a.rs"},"tool_response":{"type":"text","file":{"filePath":"/tmp/a.rs","content":"hello world content here","numLines":1,"startLine":1,"totalLines":1}}}"#;

#[test]
fn hook_scripts_delegate_stdin_to_the_binary() {
    if !have("bash") {
        return;
    }
    for (script, sub) in [
        ("pretooluse.sh", "pre-tool-use"),
        ("posttooluse.sh", "post-tool-use"),
        ("subagent-stop.sh", "subagent-stop"),
    ] {
        let dir = tmp();
        let calls = run_hook(script, READ_PAYLOAD, &dir);
        assert_eq!(calls, [format!("hook {sub}")], "{script}");
        let stdin = std::fs::read_to_string(dir.join("stdin.log")).unwrap();
        assert_eq!(stdin, READ_PAYLOAD, "{script} must pass stdin through untouched");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Run a repo hook script against the real binary in a throwaway HOME. Most
/// scripts look for `$HOME/.claude/squeez/bin/squeez`, so it is linked there.
fn run_real(script: &str, payload: &str, home: &Path) -> String {
    let bin = home.join(".claude/squeez/bin");
    std::fs::create_dir_all(&bin).unwrap();
    #[cfg(unix)]
    let _ = std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_squeez"), bin.join("squeez"));
    let mut child = Command::new("bash")
        .arg(repo_hook(script))
        .env("SQUEEZ_BIN", env!("CARGO_BIN_EXE_squeez"))
        .env("HOME", home)
        .env_remove("SQUEEZ_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn hook script");
    child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{script} exited nonzero");
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn pretooluse_real_binary_wraps_bash_with_shell_quoting() {
    if !have("bash") {
        return;
    }
    let home = tmp();
    let out = run_real(
        "pretooluse.sh",
        r#"{"tool_name":"Bash","tool_input":{"command":"echo 'a b' $HOME","description":"d"}}"#,
        &home,
    );
    let v = squeez::json_util::parse_value(&out).expect("hook printed JSON");
    let input = v.get("hookSpecificOutput").unwrap().get("updatedInput").unwrap();
    let want = format!("{} wrap 'echo '\"'\"'a b'\"'\"' $HOME'", env!("CARGO_BIN_EXE_squeez"));
    assert_eq!(input.get_str("command"), want);
    assert_eq!(input.get_str("description"), "d", "other fields survive");
    // Risky commands are left alone: no output at all.
    let out = run_real("pretooluse.sh", r#"{"tool_name":"Bash","tool_input":{"command":"rm -rf /tmp/x"}}"#, &home);
    assert!(out.trim().is_empty(), "{out}");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn posttooluse_real_binary_records_the_read() {
    if !have("bash") {
        return;
    }
    let home = tmp();
    run_real("posttooluse.sh", READ_PAYLOAD, &home);
    let ctx = std::fs::read_to_string(home.join(".claude/squeez/sessions/context.json"))
        .expect("track-result wrote context.json");
    assert!(ctx.contains("/tmp/a.rs"), "{ctx}");
    let _ = std::fs::remove_dir_all(&home);
}

// ── subagent-stop.sh ─────────────────────────────────────────────────────────

#[test]
fn subagent_stop_real_binary_releases_a_slot_and_records_the_message() {
    if !have("bash") {
        return;
    }
    let home = tmp();
    let sessions = home.join(".claude/squeez/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(sessions.join("inflight_agents"), "100\n200").unwrap();
    run_real(
        "subagent-stop.sh",
        r#"{"last_assistant_message":"done: reviewed src/lib.rs","agent_id":"a1"}"#,
        &home,
    );
    assert_eq!(
        std::fs::read_to_string(sessions.join("inflight_agents")).unwrap(),
        "200",
        "the oldest stamp is released"
    );
    let ctx = std::fs::read_to_string(sessions.join("context.json")).expect("context.json");
    assert!(ctx.contains("src/lib.rs"), "sub-agent message reached SessionContext: {ctx}");
    let _ = std::fs::remove_dir_all(&home);
}

// ── statusline.sh ────────────────────────────────────────────────────────────

#[test]
fn statusline_real_binary_renders_the_session() {
    if !have("bash") {
        return;
    }
    let home = tmp();
    assert_eq!(run_real("statusline.sh", "{}", &home), "", "no session: no line");
    let sessions = home.join(".claude/squeez/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(sessions.join("current.json"), r#"{"tokens_saved":5,"total_calls":87}"#).unwrap();
    std::fs::write(sessions.join("context.json"), r#"{"agent_spawns":2,"tokens_bash":4}"#).unwrap();
    assert_eq!(
        run_real("statusline.sh", "{}", &home),
        "squeez | -5tk 125% | 87 calls | 2 agents\n"
    );
    let _ = std::fs::remove_dir_all(&home);
}

// ── session-start.sh ─────────────────────────────────────────────────────────

#[test]
fn session_start_warns_only_when_settings_lost_the_hooks() {
    if !have("bash") {
        return;
    }
    let home = tmp();
    // Skip the once-a-day update check: no network from a test.
    std::fs::create_dir_all(home.join(".claude/squeez")).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    std::fs::write(home.join(".claude/squeez/.update-check-ts"), now.to_string()).unwrap();
    let settings = home.join(".claude/settings.json");
    let warning = "WARNING: hooks not registered";

    std::fs::write(&settings, r#"{"hooks":{"PreToolUse":[{"hooks":[{"command":"other"}]}]}}"#).unwrap();
    assert!(run_real("session-start.sh", "", &home).contains(warning));

    std::fs::write(
        &settings,
        r#"{"PreToolUse":[{"hooks":[{"command":"bash ~/.claude/squeez/hooks/pretooluse.sh"}]}]}"#,
    )
    .unwrap();
    assert!(!run_real("session-start.sh", "", &home).contains(warning));
    let _ = std::fs::remove_dir_all(&home);
}

// ── copilot-*.sh ─────────────────────────────────────────────────────────────

#[test]
fn copilot_hooks_real_binary_wrap_and_record_under_copilot_state() {
    if !have("bash") {
        return;
    }
    let home = tmp();
    let out = run_real("copilot-pretooluse.sh", r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#, &home);
    let v = squeez::json_util::parse_value(&out).expect("hook printed JSON");
    let cmd = v.get("hookSpecificOutput").unwrap().get("updatedInput").unwrap().get_str("command").to_string();
    assert!(cmd.ends_with("/.claude/squeez/bin/squeez wrap 'git status'"), "{cmd}");
    // No spawn ceiling on Copilot: an Agent call is not Copilot's to gate.
    assert_eq!(run_real("copilot-pretooluse.sh", r#"{"tool_name":"Task","tool_input":{"prompt":"x"}}"#, &home), "");

    run_real("copilot-posttooluse.sh", READ_PAYLOAD, &home);
    let ctx = std::fs::read_to_string(home.join(".copilot/squeez/sessions/context.json"))
        .expect("Copilot state written under ~/.copilot/squeez");
    assert!(ctx.contains("/tmp/a.rs"), "{ctx}");
    assert!(!home.join(".claude/squeez/sessions/inflight_agents").exists());
    let _ = std::fs::remove_dir_all(&home);
}

// ── no hook needs an interpreter ─────────────────────────────────────────────

#[test]
fn no_hook_script_runs_python() {
    for entry in std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("hooks")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("sh") {
            continue;
        }
        let body = std::fs::read_to_string(&path).unwrap();
        for line in body.lines().filter(|l| !l.trim_start().starts_with('#')) {
            assert!(!line.contains("python") && !line.contains("SQUEEZ_PY"), "{}: {line}", path.display());
        }
    }
}
