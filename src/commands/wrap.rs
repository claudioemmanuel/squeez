use crate::config::Config;
use crate::filter;
use crate::{json_util, session};
use std::io::Read;
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::sync::atomic::{AtomicI32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
static CHILD_PID: AtomicI32 = AtomicI32::new(-1);

/// Returns a `Command` pre-configured to run `cmd` through the platform shell.
/// Unix/Git Bash: `sh -c <cmd>`
/// Windows native: `cmd /C <cmd>`
fn shell_command(cmd: &str) -> Command {
    #[cfg(windows)]
    {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd]);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.args(["-c", cmd]);
        c
    }
}

pub fn run(cmd_str: &str) -> i32 {
    #[cfg(unix)]
    setup_signals();
    let config = Config::load();

    if !config.enabled || config.is_bypassed(cmd_str) || is_streaming(cmd_str) {
        return passthrough(cmd_str);
    }

    let start = Instant::now();

    // Spawn via platform shell to handle pipes, &&, redirections, builtins
    let mut cmd = shell_command(cmd_str);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("squeez: {}", e);
            return 1;
        }
    };

    // Store PID for signal forwarding (Unix only)
    #[cfg(unix)]
    CHILD_PID.store(child.id() as i32, Ordering::SeqCst);

    // Drain stdout/stderr on background threads to prevent pipe-buffer deadlock.
    // This MUST happen before the try_wait loop — if we wait first, the child can
    // block writing to a full pipe and never exit, causing a deadlock.
    let stdout_pipe = match child.stdout.take() {
        Some(p) => p,
        None => {
            eprintln!("squeez: failed to capture stdout");
            return 1;
        }
    };
    let stderr_pipe = match child.stderr.take() {
        Some(p) => p,
        None => {
            eprintln!("squeez: failed to capture stderr");
            return 1;
        }
    };
    // Cap capture at 10 MB per stream to prevent OOM on runaway output.
    const MAX_CAPTURE: u64 = 10 * 1024 * 1024;
    let stdout_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        stdout_pipe.take(MAX_CAPTURE).read_to_end(&mut buf).ok();
        buf
    });
    let stderr_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        stderr_pipe.take(MAX_CAPTURE).read_to_end(&mut buf).ok();
        buf
    });

    // Poll for exit with 120s timeout
    let timeout = Duration::from_secs(120);
    let exit_code = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s.code().unwrap_or(1),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(-(child.id() as i32), libc::SIGTERM);
                        std::thread::sleep(Duration::from_millis(200));
                    }
                    let _ = child.kill();
                    eprintln!("squeez: command timed out after 120s");
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
                    return 124;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                eprintln!("squeez: wait error: {}", e);
                return 1;
            }
        }
    };

    // Pipes are closed (child exited), join safely
    let stdout_bytes = stdout_thread.join().unwrap_or_default();
    let stderr_bytes = stderr_thread.join().unwrap_or_default();

    let elapsed_ms = start.elapsed().as_millis();

    // Merge stderr + stdout
    let mut combined = String::new();
    combined.push_str(&String::from_utf8_lossy(&stderr_bytes));
    combined.push_str(&String::from_utf8_lossy(&stdout_bytes));

    let input_tokens = combined.len() / 4;
    let lines: Vec<String> = combined.lines().map(String::from).collect();

    let compressed = filter::compress(cmd_str, lines, &config);
    let output_str = compressed.join("\n");
    let output_tokens = output_str.len() / 4;

    // ── Artifact capture + session tracking ───────────────────────────────
    let files      = extract_file_paths(&combined);
    let errors     = extract_errors(&combined);
    let git_events = extract_git_events(cmd_str, &combined);
    let test_sum   = extract_test_summary(&combined);

    let compact_warning = record_bash_event(
        cmd_str, input_tokens, output_tokens, &files, &errors, &git_events, &test_sum, &config,
    );

    let reduction = if input_tokens > 0 {
        100usize.saturating_sub(output_tokens * 100 / input_tokens)
    } else {
        0
    };

    let cmd_name = cmd_str.split_whitespace().next().unwrap_or("cmd");

    if config.show_header {
        println!(
            "# squeez [{}] {}→{} tokens (-{}%) {}ms",
            cmd_name, input_tokens, output_tokens, reduction, elapsed_ms
        );
        if let Some(ref warning) = compact_warning {
            println!("{}", warning);
        }
    }
    if !output_str.is_empty() {
        println!("{}", output_str);
    }

    exit_code
}

fn passthrough(cmd: &str) -> i32 {
    let status = shell_command(cmd)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("squeez: {}", e);
            std::process::exit(1);
        });
    status.code().unwrap_or(1)
}

fn is_streaming(cmd: &str) -> bool {
    let name = cmd.split_whitespace().next().unwrap_or("");
    let follow_cmds = ["tail", "docker", "kubectl"];
    follow_cmds.iter().any(|c| name.contains(c))
        && cmd.split_whitespace().any(|a| a == "-f" || a == "--follow")
}

#[cfg(unix)]
fn setup_signals() {
    unsafe {
        libc::signal(libc::SIGTERM, forward_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, forward_signal as *const () as libc::sighandler_t);
    }
}

#[cfg(unix)]
extern "C" fn forward_signal(sig: libc::c_int) {
    let pid = CHILD_PID.load(Ordering::SeqCst);
    if pid > 0 {
        unsafe {
            libc::kill(-pid, sig);
        }
    }
}

// ── Artifact extraction ────────────────────────────────────────────────────

pub fn extract_file_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for word in text.split_whitespace() {
        let w = word.trim_matches(|c| c == ',' || c == ':' || c == '(' || c == ')' || c == '\'' || c == '"');
        if looks_like_path(w) && seen.insert(w.to_string()) {
            out.push(w.to_string());
        }
    }
    out
}

fn looks_like_path(s: &str) -> bool {
    s.contains('/')
        && !s.starts_with("http")
        && !s.starts_with("//")
        && s.len() > 4
        && s.len() < 160
        && s.chars().all(|c| c.is_alphanumeric() || "/_.-:".contains(c))
        && s.contains('.')
}

pub fn extract_errors(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("error:") || t.starts_with("Error:")
                || t.starts_with("error[") || t.starts_with("FAILED")
                || t.starts_with("fatal:") || t.starts_with("panic:")
        })
        .take(3)
        .map(|l| l.trim().chars().take(120).collect())
        .collect()
}

pub fn extract_test_summary(text: &str) -> String {
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with("test result:") { return l.chars().take(80).collect(); }
        if l.contains(" passed") && l.contains(" failed") { return l.chars().take(80).collect(); }
        if l.starts_with("PASSED") || l.starts_with("FAILED") { return l.chars().take(80).collect(); }
    }
    String::new()
}

/// Public wrapper for tests (private logic is `extract_git_events`).
pub fn extract_git_events_pub(cmd: &str, text: &str) -> Vec<String> {
    extract_git_events(cmd, text)
}

fn extract_git_events(cmd: &str, text: &str) -> Vec<String> {
    let name = cmd.split_whitespace().next().unwrap_or("");
    let is_git = name == "git" || name.ends_with("/git");
    if !is_git { return Vec::new(); }
    text.lines()
        .filter(|l| {
            let t = l.trim();
            t.chars().take(7).count() == 7 && t.chars().take(7).all(|c| c.is_ascii_hexdigit())
        })
        .take(5)
        .map(|l| l.trim().chars().take(100).collect())
        .collect()
}

fn record_bash_event(
    cmd: &str,
    in_tk: usize,
    out_tk: usize,
    files: &[String],
    errors: &[String],
    git: &[String],
    test_summary: &str,
    config: &Config,
) -> Option<String> {
    let dir = session::sessions_dir();
    let mut current = session::CurrentSession::load(&dir)?;

    current.total_tokens += out_tk as u64;

    let event = format!(
        "{{\"type\":\"bash\",\"cmd\":\"{}\",\"in_tk\":{},\"out_tk\":{},\
\"files\":{},\"errors\":{},\"git\":{},\"test_summary\":\"{}\",\"ts\":{}}}",
        json_util::escape_str(cmd),
        in_tk, out_tk,
        json_util::str_array(files),
        json_util::str_array(errors),
        json_util::str_array(git),
        json_util::escape_str(test_summary),
        session::unix_now(),
    );
    session::append_event(&dir, &current.session_file, &event);

    let warning = if !current.compact_warned
        && current.total_tokens >= config.compact_threshold_tokens
    {
        let budget = config.compact_threshold_tokens * 5 / 4;
        let pct = current.total_tokens * 100 / budget.max(1);
        current.compact_warned = true;
        Some(format!(
            "⚠️  squeez: session ~{}K tokens ({}% of budget). Run /compact to free context.\n    Artifacts: {} files touched, {} errors seen.",
            current.total_tokens / 1000,
            pct,
            files.len(),
            errors.len(),
        ))
    } else {
        None
    };

    current.save(&dir);
    warning
}
