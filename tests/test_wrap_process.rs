//! How `wrap` treats the processes behind a command: what it returns when one
//! outlives the shell, what a timeout kills, and how the command text reaches
//! the shell. The `#[cfg(windows)]` half runs on the Windows CI job, against
//! git-bash — the shell every supported host hands its commands to there.

use std::process::{Command, Output};
use std::time::{Duration, Instant};

fn bin() -> String {
    env!("CARGO_BIN_EXE_squeez").to_string()
}

/// Unique, isolated SQUEEZ_DIR so wrap never touches the real ~/.claude state.
fn tmp_squeez_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "squeez_wrap_proc_{}_{}",
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

/// Runs `squeez wrap <cmd>` to completion — including EOF on its stdout and
/// stderr, which is what a host waits for — and reports how long that took.
fn wrap(label: &str, cmd: &str, timeout_secs: &str) -> (Output, Duration) {
    let dir = tmp_squeez_dir(label);
    let start = Instant::now();
    let out = Command::new(bin())
        .args(["wrap", cmd])
        .env("SQUEEZ_DIR", &dir)
        .env("SQUEEZ_WRAP_TIMEOUT_SECS", timeout_secs)
        .output()
        .unwrap();
    let elapsed = start.elapsed();
    std::fs::remove_dir_all(&dir).ok();
    (out, elapsed)
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

// ── Issue #261 ──────────────────────────────────────────────────────────────

#[test]
fn timeout_keeps_the_output_printed_before_the_hang() {
    let (out, elapsed) = wrap("timeout_partial", "echo before-the-hang; sleep 30", "1");

    assert_eq!(out.status.code(), Some(124));
    assert!(stderr(&out).contains("timed out after 1s"), "got: {}", stderr(&out));
    assert!(stdout(&out).contains("before-the-hang"), "got: {}", stdout(&out));
    assert!(elapsed < Duration::from_secs(15), "returned only after {elapsed:?}");
}

#[test]
fn a_background_process_holding_the_output_does_not_outlast_the_ceiling() {
    // The shell exits at once; `sleep` keeps the capture pipes open. wrap must
    // return at the ceiling with the shell's own exit code and output, not
    // when the background process finally exits.
    let (out, elapsed) = wrap("drain_ceiling", "sleep 30 & echo started", "2");

    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("started"), "got: {}", stdout(&out));
    assert!(stderr(&out).contains("still holds its output"), "got: {}", stderr(&out));
    assert!(elapsed < Duration::from_secs(15), "returned only after {elapsed:?}");
}

#[cfg(windows)]
#[test]
fn a_redirected_daemon_does_not_hold_the_callers_pipe() {
    // The daemon has no fd on the capture pipes, so wrap itself returns at
    // once. What used to hang was the CALLER: squeez's own std handles leaked
    // into the daemon, so EOF arrived only when it exited.
    let (out, elapsed) = wrap(
        "daemon_redirected",
        "sleep 25 > /dev/null 2>&1 & echo started",
        "60",
    );

    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("started"), "got: {}", stdout(&out));
    assert!(elapsed < Duration::from_secs(15), "EOF only after {elapsed:?}");
}

#[cfg(windows)]
fn msys_sleepers(marker: &str) -> String {
    let script = format!(
        "(Get-CimInstance Win32_Process | Where-Object {{ $_.Name -eq 'sleep.exe' -and \
         $_.CommandLine -like '*{marker}*' }} | Measure-Object).Count"
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[cfg(windows)]
#[test]
fn timeout_kills_a_program_the_msys_shell_started() {
    // `sleep` is an MSYS program: the shell's exec leaves it without a Windows
    // parent link, so `taskkill /T` alone walked straight past it.
    let (out, _) = wrap("timeout_msys_child", "sleep 31337; echo done", "1");
    assert_eq!(out.status.code(), Some(124));

    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(msys_sleepers("31337"), "0", "the MSYS child survived the timeout");
}

// ── Issue #262 ──────────────────────────────────────────────────────────────

#[cfg(windows)]
#[test]
fn a_command_past_the_msys_argument_limit_runs_to_its_last_statement() {
    let cmd = format!(": {}; echo tail-marker", "x".repeat(9000));
    let (out, _) = wrap("long_command", &cmd, "30");

    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("tail-marker"), "got: {}", stdout(&out));
}

#[cfg(windows)]
#[test]
fn backslash_pairs_reach_the_shell_intact() {
    let (out, _) = wrap("backslashes", r"printf '%s\n' 'a\\\\b'", "30");

    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains(r"a\\\\b"), "got: {}", stdout(&out));
}
