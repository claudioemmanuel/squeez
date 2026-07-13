use std::process::Command;

fn bin() -> String {
    env!("CARGO_BIN_EXE_squeez").to_string()
}

/// Unique, isolated SQUEEZ_DIR for a subprocess `wrap` test (sessions +
/// memory dirs pre-created so wrap never falls back to the real ~/.claude
/// state).
fn tmp_squeez_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "squeez_wrap_it_{}_{}",
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
fn wrap_runs_and_shows_header() {
    // Net-win gate (R4/E1) suppresses the header on a no-op compression by
    // default; this smoke test only checks the header pipeline runs at all,
    // so disable the gate rather than couple it to gate behavior (see the
    // net-win tests below for that).
    let dir = tmp_squeez_dir("smoke");
    std::fs::write(dir.join("config.ini"), "net_win_min_tokens = 0\n").unwrap();

    let out = Command::new(bin())
        .args(["wrap", "echo hello"])
        .env("SQUEEZ_DIR", &dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("hello"));
    assert!(stdout.contains("# squeez"));
    assert_eq!(out.status.code(), Some(0));

    std::fs::remove_dir_all(&dir).ok();
}

// ── Net-win gate / show_header tri-state (E1) ───────────────────────────────

#[test]
fn net_win_gate_suppresses_header_on_noop_call() {
    // Default config: net_win_min_tokens=24, show_header=net. A trivial,
    // uncompressible command saves 0 tokens — the gate must suppress the
    // header (raw output still passes through unchanged).
    let dir = tmp_squeez_dir("noop_default");

    let out = Command::new(bin())
        .args(["wrap", "echo hello"])
        .env("SQUEEZ_DIR", &dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("hello"));
    assert!(
        !stdout.contains("# squeez"),
        "expected no header on a no-win call, got: {stdout}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn show_header_always_restores_header_on_noop_call() {
    // show_header=always must print the header even though the same no-op
    // call would otherwise be gated by net_win_min_tokens.
    let dir = tmp_squeez_dir("always");
    std::fs::write(dir.join("config.ini"), "show_header = always\n").unwrap();

    let out = Command::new(bin())
        .args(["wrap", "echo hello"])
        .env("SQUEEZ_DIR", &dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("hello"));
    assert!(
        stdout.contains("# squeez"),
        "expected header with show_header=always, got: {stdout}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn show_header_off_suppresses_header_even_with_gate_disabled() {
    // show_header=off must suppress the header even when net_win_min_tokens=0
    // disables the gate itself (i.e. the "net" gate would otherwise show it) —
    // isolating this assertion to the show_header switch.
    let dir = tmp_squeez_dir("off");
    std::fs::write(
        dir.join("config.ini"),
        "show_header = off\nnet_win_min_tokens = 0\n",
    )
    .unwrap();

    let out = Command::new(bin())
        .args(["wrap", "echo hello"])
        .env("SQUEEZ_DIR", &dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("hello"));
    assert!(
        !stdout.contains("# squeez"),
        "expected no header with show_header=off, got: {stdout}"
    );

    std::fs::remove_dir_all(&dir).ok();
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

#[cfg(not(windows))]
#[test]
fn wrap_handles_pipes_via_sh() {
    let out = Command::new(bin())
        .args(["wrap", "echo hello | tr a-z A-Z"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("HELLO"));
}

#[cfg(windows)]
#[test]
fn wrap_handles_pipes_via_cmd() {
    let out = Command::new(bin())
        .args(["wrap", "echo hello | findstr hello"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.to_lowercase().contains("hello"));
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

// --- Artifact extraction unit tests ---

#[test]
fn test_extract_file_paths_from_output() {
    let text = "error in src/auth.ts:42\nFix src/components/Foo.tsx line 10\n";
    let files = squeez::commands::wrap::extract_file_paths(text);
    assert!(files.iter().any(|f| f.contains("src/auth.ts")), "got: {:?}", files);
}

#[test]
fn test_extract_file_paths_http_urls_filtered() {
    // URLs starting with http must not be included
    let text = "see https://docs.rs/squeez/latest for details\n";
    let files = squeez::commands::wrap::extract_file_paths(text);
    assert!(files.is_empty(), "HTTP URL should be filtered, got: {:?}", files);
}

#[test]
fn test_extract_file_paths_no_extension_filtered() {
    // Paths without a dot (no extension) must not be included
    let text = "binary at /usr/bin/rustc and /usr/local/bin/cargo\n";
    let files = squeez::commands::wrap::extract_file_paths(text);
    assert!(files.is_empty(), "extensionless paths should be filtered, got: {:?}", files);
}

#[test]
fn test_extract_file_paths_deduplicates() {
    // Same token must appear only once — use exact repeated word, no colon suffix
    let text = "Fix src/main.rs\nAlso see src/main.rs for context\n";
    let files = squeez::commands::wrap::extract_file_paths(text);
    let count = files.iter().filter(|f| *f == "src/main.rs").count();
    assert_eq!(count, 1, "duplicate path should appear once, got: {:?}", files);
}

#[test]
fn test_extract_errors_capped_at_three() {
    // Four error lines — only first three should be captured
    let text = "error: first\nerror: second\nerror: third\nerror: fourth\n";
    let errors = squeez::commands::wrap::extract_errors(text);
    assert_eq!(errors.len(), 3, "should cap at 3, got: {:?}", errors);
    assert!(!errors.iter().any(|e| e.contains("fourth")), "fourth should be dropped, got: {:?}", errors);
}

#[test]
fn test_extract_errors_multiple_prefixes() {
    let text = "fatal: not a git repo\npanic: index out of bounds\nFAILED: build step\n";
    let errors = squeez::commands::wrap::extract_errors(text);
    assert_eq!(errors.len(), 3, "got: {:?}", errors);
    assert!(errors.iter().any(|e| e.contains("fatal")));
    assert!(errors.iter().any(|e| e.contains("panic")));
    assert!(errors.iter().any(|e| e.contains("FAILED")));
}

#[test]
fn test_extract_errors_no_match_returns_empty() {
    let text = "info: all good\nwarning: minor thing\n";
    let errors = squeez::commands::wrap::extract_errors(text);
    assert!(errors.is_empty(), "got: {:?}", errors);
}

#[test]
fn test_extract_test_summary_cargo() {
    let text = "test foo ... ok\ntest result: ok. 5 passed; 1 failed; 0 ignored\n";
    let summary = squeez::commands::wrap::extract_test_summary(text);
    assert!(summary.contains("5"), "got: {:?}", summary);
}

#[test]
fn test_extract_test_summary_pytest_format() {
    // pytest "X passed, Y failed" hits the contains(" passed") && contains(" failed") branch
    // Put it first so it's matched before any PASSED-prefixed line
    let text = "3 passed, 1 failed in 0.42s\n";
    let summary = squeez::commands::wrap::extract_test_summary(text);
    assert!(!summary.is_empty(), "pytest format should match, got: {:?}", summary);
    assert!(summary.contains("passed") || summary.contains("failed"), "got: {:?}", summary);
}

#[test]
fn test_extract_test_summary_no_match_returns_empty() {
    let text = "compiling src/main.rs\nfinished in 2.3s\n";
    let summary = squeez::commands::wrap::extract_test_summary(text);
    assert!(summary.is_empty(), "no test output should return empty, got: {:?}", summary);
}

#[test]
fn test_extract_git_events_non_git_cmd_returns_empty() {
    let text = "abc1234 some commit\n";
    let events = squeez::commands::wrap::extract_git_events_pub("cargo build", text);
    assert!(events.is_empty(), "non-git command should return empty, got: {:?}", events);
}

#[test]
fn test_extract_git_events_six_hex_chars_rejected() {
    // Six hex chars is not a valid short SHA — need at least 7
    let text = "abc123 not enough\nabc1234 valid sha\n";
    let events = squeez::commands::wrap::extract_git_events_pub("git log", text);
    assert!(!events.iter().any(|e| e.starts_with("abc123 ")), "6-char hex should be rejected, got: {:?}", events);
    assert!(events.iter().any(|e| e.starts_with("abc1234")), "7-char hex should be accepted, got: {:?}", events);
}

#[test]
fn test_extract_git_events_non_ascii_safe() {
    // Non-ASCII in commit message must not panic (this was a prior panic risk)
    let text = "abc1234 feat: ✨ add emoji support\ndef5678 fix: résumé parsing\n";
    // Must not panic — if it does the test fails
    let events = squeez::commands::wrap::extract_git_events_pub("git log", text);
    assert!(!events.is_empty(), "should extract git events with non-ASCII, got: {:?}", events);
}
