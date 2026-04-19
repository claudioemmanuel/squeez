use crate::config::Config;
use crate::context;
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

    // ── Context engine pre-pass ────────────────────────────────────────
    let sessions_dir_pp = session::sessions_dir();
    let used_tokens = session::CurrentSession::load(&sessions_dir_pp)
        .map(|c| c.total_tokens)
        .unwrap_or(0);
    let (mut ctx, intensity, eff_cfg) =
        context::pre_pass(&config, &sessions_dir_pp, used_tokens);

    // Optional cross-call hint for raw cat/head/tail of seen files
    if let Some(hint) = context::cache::raw_read_hint(&ctx, cmd_str) {
        println!("{}", hint);
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

    // ── Summarize fallback for huge outputs (pre-handler) ──────────────
    // Decision based on raw line count so handlers can't hide huge inputs.
    let mut compressed = if context::summarize::should_apply(&lines, &eff_cfg) {
        let fmt = {
            use context::summarize::SummaryFormat;
            use context::intensity::Intensity;
            match config.summary_format.as_str() {
                "prose"      => SummaryFormat::Prose,
                "structured" => SummaryFormat::Structured,
                _            => if intensity == Intensity::Ultra {
                    SummaryFormat::Structured
                } else {
                    SummaryFormat::Prose
                },
            }
        };
        context::summarize::apply_with_format(lines, cmd_str, fmt)
    } else {
        filter::compress(cmd_str, lines, &eff_cfg)
    };

    // ── Redundancy short-circuit ───────────────────────────────────────
    let mut redundancy_hit = false;
    if eff_cfg.redundancy_cache_enabled {
        if let Some(hit) = context::redundancy::check(&ctx, &compressed) {
            compressed = vec![match hit.similarity {
                None => format!(
                    "[squeez: identical to {} at bash#{} — re-run with --no-squeez]",
                    hit.short_hash, hit.call_n
                ),
                Some(j) => format!(
                    "[squeez: ~{}% similar to {} at bash#{} — re-run with --no-squeez]",
                    (j * 100.0).round() as u32,
                    hit.short_hash,
                    hit.call_n
                ),
            }];
            redundancy_hit = true;
        }
    }

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
        let intensity_tag = if config.adaptive_intensity {
            format!(" [adaptive: {}]", intensity.as_str())
        } else {
            String::new()
        };
        // Token economy: burn rate prediction
        let budget_tag = crate::economy::burn_rate::pressure_warning(&ctx, &config)
            .or_else(|| {
                crate::economy::burn_rate::calls_remaining(&ctx, &config)
                    .map(|r| crate::economy::burn_rate::format_pressure_header(r))
            })
            .unwrap_or_default();
        let budget_tag = if budget_tag.is_empty() {
            String::new()
        } else {
            format!(" {}", budget_tag)
        };
        // Token economy: agent cost warning
        let agent_tag = crate::economy::agent_tracker::agent_cost_warning(&ctx, &config)
            .map(|w| format!(" {}", w))
            .unwrap_or_default();
        println!(
            "# squeez [{}] {}→{} tokens (-{}%) {}ms{}{}{}",
            cmd_name, input_tokens, output_tokens, reduction, elapsed_ms,
            intensity_tag, budget_tag, agent_tag
        );
        if let Some(ref warning) = compact_warning {
            println!("{}", warning);
        }
    }
    if !output_str.is_empty() {
        println!("{}", output_str);
    }

    // ── Context engine post-pass ───────────────────────────────────────
    if config.context_cache_enabled && !redundancy_hit {
        context::redundancy::record(&mut ctx, cmd_str, &compressed);
    } else if config.context_cache_enabled {
        // still bump the call counter so future calls reference the right index
        ctx.next_call_n();
    }
    if config.context_cache_enabled {
        let access = detect_file_access(cmd_str);
        for f in &files {
            ctx.note_file(f, access.clone());
        }
        ctx.note_errors(&errors);
        ctx.note_git(&git_events);
        ctx.note_tool_tokens("Bash", input_tokens as u64);
        // Token economy: record burn rate
        ctx.note_burn(output_tokens as u64);
        ctx.save(&sessions_dir_pp);
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

/// Infer the file access type from the shell command name.
/// Defaults to `Read` when ambiguous (most bash-extracted file paths are reads).
fn detect_file_access(cmd: &str) -> crate::context::cache::FileAccess {
    use crate::context::cache::FileAccess;
    let first = cmd.split_whitespace().next().unwrap_or("");
    let name = first.rsplit('/').next().unwrap_or(first);
    match name {
        "rm" | "unlink" | "rmdir" => FileAccess::Deleted,
        "tee" => FileAccess::Write,
        "cat" | "head" | "tail" | "less" | "more" | "bat" => FileAccess::Read,
        _ => {
            // Redirection operators in the full command → write.
            if cmd.contains(" > ") || cmd.contains(" >> ") {
                FileAccess::Write
            } else {
                FileAccess::Read
            }
        }
    }
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

const MAX_FILE_PATHS: usize = 100;

pub fn extract_file_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for word in text.split_whitespace() {
        if out.len() >= MAX_FILE_PATHS {
            break;
        }
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
    if in_tk > out_tk {
        current.tokens_saved += (in_tk - out_tk) as u64;
    }

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

    let budget = config.compact_threshold_tokens * 5 / 4;
    let pct = current.total_tokens * 100 / budget.max(1);

    let warning = if !current.compact_warned
        && current.total_tokens >= config.compact_threshold_tokens
    {
        current.compact_warned = true;
        // Load context for per-tool breakdown
        let ctx = crate::context::cache::SessionContext::load(&dir);
        Some(format!(
            "⚠️  squeez: session ~{}K tokens ({}% of budget). Run /compact to free context.\n    Token breakdown: Bash {}K | Read {}K | Grep {}K | Other {}K",
            current.total_tokens / 1000,
            pct,
            ctx.tokens_bash / 1000,
            ctx.tokens_read / 1000,
            ctx.tokens_grep / 1000,
            ctx.tokens_other / 1000,
        ))
    } else if !current.state_warned {
        // Tier-2: State-First Pattern suggestion at critical pressure.
        let critical = if pct >= 90 {
            true
        } else {
            let ctx = crate::context::cache::SessionContext::load(&dir);
            crate::economy::burn_rate::calls_remaining(&ctx, config)
                .map(|r| r <= config.state_warn_calls)
                .unwrap_or(false)
        };
        if critical {
            current.state_warned = true;
            Some(format!(
                "🚨 squeez: context critical ({}%) — save state before clearing:\n\
                 \n\
                 Write `.claude/session_state.md` with:\n\
                 ## Current Objective\n\
                 <what you're solving now>\n\
                 ## Files Read\n\
                 <paths + what was learned>\n\
                 ## Decisions Taken\n\
                 <why approach X not Y>\n\
                 ## Next Steps\n\
                 <immediate plan>\n\
                 \n\
                 Then run `/clear` to reset context (or `/compact [describe focus area]` for a focused summary).",
                pct.min(100),
            ))
        } else {
            None
        }
    } else {
        None
    };

    current.save(&dir);
    warning
}
