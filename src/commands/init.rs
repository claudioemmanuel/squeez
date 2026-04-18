use std::path::Path;

use crate::{
    commands::{compress_md, persona},
    config::Config,
    json_util, memory,
    session::{self, CurrentSession},
};

/// Entry point called from main.rs: `squeez init`
pub fn run() -> i32 {
    let cfg = Config::load();
    let sessions = session::sessions_dir();
    let mem = session::memory_dir();
    let _ = std::fs::create_dir_all(&sessions);
    let _ = std::fs::create_dir_all(&mem);
    let code = run_with_dirs(&sessions, &mem, &cfg);
    // Delegate host-specific memory injection (CLAUDE.md) to the adapter.
    if let Some(adapter) = crate::hosts::find("claude-code") {
        let _ = adapter.inject_memory(&cfg, &[]);
    }
    // Warn if CLAUDE.md is larger than ~1K tokens (research recommendation).
    check_claude_md_size();
    code
}

/// Entry point called from main.rs: `squeez init --copilot`
/// Same as run() but also injects the memory banner into
/// ~/.copilot/copilot-instructions.md so Copilot CLI picks it up
/// at every session start (no hook system required).
pub fn run_copilot() -> i32 {
    let home = crate::session::home_dir();
    // Honour SQUEEZ_DIR override, default to ~/.copilot/squeez
    let base = std::env::var("SQUEEZ_DIR")
        .unwrap_or_else(|_| format!("{}/.copilot/squeez", home));
    let sessions = std::path::PathBuf::from(&base).join("sessions");
    let mem = std::path::PathBuf::from(&base).join("memory");
    let _ = std::fs::create_dir_all(&sessions);
    let _ = std::fs::create_dir_all(&mem);

    // Load config from the copilot squeez dir
    let cfg = load_config_from(&base);

    let code = run_with_dirs(&sessions, &mem, &cfg);

    // Delegate memory injection to the Copilot CLI adapter.
    let summaries = memory::read_last_n(&mem, 3);
    if let Some(adapter) = crate::hosts::find("copilot") {
        let _ = adapter.inject_memory(&cfg, &summaries);
    }
    // Suppress unused-variable warning when copilot adapter is absent at compile time.
    let _ = home;

    // Auto-compress copilot-instructions.md after we just rewrote it.
    if cfg.auto_compress_md {
        let _ = compress_md::run_all_quietly();
    }

    code
}

fn load_config_from(base: &str) -> Config {
    let path = format!("{}/config.ini", base);
    std::fs::read_to_string(&path)
        .map(|s| Config::from_str(&s))
        .unwrap_or_default()
}

/// Generic entry point for any `HostAdapter`. Resolves the adapter by slug,
/// prepares its data directory (sessions/memory), runs the shared session
/// finalize + banner flow, then delegates host-specific memory injection to
/// the adapter.
///
/// Backs `squeez init --host=<name>` in `main.rs`.
pub fn run_for_host(host_name: &str) -> i32 {
    let adapter = match crate::hosts::find(host_name) {
        Some(a) => a,
        None => {
            eprintln!("squeez init: unknown host '{}'", host_name);
            return 1;
        }
    };
    let data_dir = adapter.data_dir();
    let sessions = data_dir.join("sessions");
    let mem = data_dir.join("memory");
    let _ = std::fs::create_dir_all(&sessions);
    let _ = std::fs::create_dir_all(&mem);
    let cfg = load_config_from(&data_dir.to_string_lossy());

    let code = run_with_dirs(&sessions, &mem, &cfg);

    let summaries = memory::read_last_n(&mem, 3);
    let _ = adapter.inject_memory(&cfg, &summaries);

    if cfg.auto_compress_md {
        let _ = compress_md::run_all_quietly();
    }

    code
}

/// Testable version with explicit directories.
pub fn run_with_dirs(sessions_dir: &Path, memory_dir: &Path, config: &Config) -> i32 {
    // 1. Finalise previous session → memory (best-effort)
    if let Some(prev) = CurrentSession::load(sessions_dir) {
        finalize(&prev, sessions_dir, memory_dir, config);
    }

    // 2. Start new session
    let now = session::unix_now();
    let new = CurrentSession {
        session_file: session::new_session_filename(),
        total_tokens: 0,
        tokens_saved: 0,
        total_calls: 0,
        compact_warned: false,
        state_warned: false,
        start_ts: now,
    };
    new.save(sessions_dir);

    // 3. Git snapshot into session log (best-effort, may fail if not in a git repo)
    let git_log = git(&["log", "--oneline", "-5"]);
    let git_status = git(&["status", "--porcelain"]);
    if !git_log.is_empty() || !git_status.is_empty() {
        let event = format!(
            "{{\"type\":\"git_snapshot\",\"log\":\"{}\",\"status\":\"{}\",\"ts\":{}}}",
            json_util::escape_str(&git_log),
            json_util::escape_str(&git_status),
            now,
        );
        session::append_event(sessions_dir, &new.session_file, &event);
    }

    // 4. Print banner to stdout (SessionStart hook captures this as context)
    let budget_k = config.compact_threshold_tokens * 5 / 4 / 1000;
    let summaries = memory::read_last_n(memory_dir, 3);
    println!("─── squeez active ─────────────────────────────────────────");
    println!(
        "Context budget: ~{}K tokens | Compression: ON | Memory: ON | Persona: {}",
        budget_k,
        persona::as_str(config.persona)
    );
    for (i, s) in summaries.iter().enumerate() {
        println!("{}", s.display_line());
        // Show next_steps only for the most recent session (index 0 = most recent).
        if i == 0 && !s.next_steps.is_empty() {
            let items: Vec<&str> = s.next_steps.iter().take(3).map(|s| s.as_str()).collect();
            println!("  Next steps: {}", items.join(" | "));
        }
    }
    let persona_text = persona::text(config.persona);
    if !persona_text.is_empty() {
        println!();
        print!("{}", persona_text);
    }
    println!("────────────────────────────────────────────────────────────");

    // 5. Auto-compress known memory files (idempotent — backup is never clobbered)
    if config.auto_compress_md {
        let _ = compress_md::run_all_quietly();
    }

    0
}

fn finalize(prev: &CurrentSession, sessions_dir: &Path, memory_dir: &Path, config: &Config) {
    // Reject empty or path-traversing session_file values (same guard as append_event).
    if prev.session_file.is_empty()
        || prev.session_file.contains('/')
        || prev.session_file.contains("..")
    {
        return;
    }
    let content = match std::fs::read_to_string(sessions_dir.join(&prev.session_file)) {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut files_touched: Vec<String> = Vec::new();
    let mut git_events: Vec<String> = Vec::new();
    let mut errors_resolved: Vec<String> = Vec::new();
    let mut test_summary = String::new();
    let mut total_in: u64 = 0;
    let mut total_out: u64 = 0;
    // Phase 1: structured fields collected in the same pass.
    let mut completed: Vec<String> = Vec::new();
    let mut next_steps: Vec<String> = Vec::new();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match json_util::extract_str(line, "type").as_deref() {
            Some("bash") => {
                total_in += json_util::extract_u64(line, "in_tk").unwrap_or(0);
                total_out += json_util::extract_u64(line, "out_tk").unwrap_or(0);
                dedup_extend(
                    &mut files_touched,
                    json_util::extract_str_array(line, "files"),
                );
                dedup_extend(
                    &mut errors_resolved,
                    json_util::extract_str_array(line, "errors"),
                );
                dedup_extend(&mut git_events, json_util::extract_str_array(line, "git"));
                if let Some(ts) = json_util::extract_str(line, "test_summary") {
                    if !ts.is_empty() {
                        test_summary = ts.clone();
                        let ts_lc = ts.to_lowercase();
                        // "test result: ok" means all tests passed (even if "0 failed" appears).
                        // "test result: failed" means the run failed.
                        let is_ok = ts_lc.contains("test result: ok")
                            || (ts_lc.contains("ok") && !ts_lc.contains("test result: failed") && !ts_lc.contains(": failed"));
                        let is_fail = ts_lc.contains("test result: failed")
                            || ts_lc.contains("error");
                        if is_ok && !is_fail {
                            let entry: String = ts.chars().take(80).collect();
                            if !completed.contains(&entry) {
                                completed.push(entry);
                            }
                        } else if is_fail {
                            let entry: String = ts.chars().take(80).collect();
                            if !next_steps.contains(&entry) {
                                next_steps.push(entry);
                            }
                        }
                    }
                }
                if let Some(cmd) = json_util::extract_str(line, "cmd") {
                    let has_errors =
                        !json_util::extract_str_array(line, "errors").is_empty();
                    if !has_errors {
                        if cmd.contains("cargo build") || cmd.contains("cargo check") {
                            let entry =
                                format!("build OK: {}", cmd.chars().take(30).collect::<String>());
                            if !completed.contains(&entry) {
                                completed.push(entry);
                            }
                        }
                        if cmd.contains("git push") || cmd.contains("git commit") {
                            let entry: String = cmd.chars().take(60).collect();
                            if !completed.contains(&entry) {
                                completed.push(entry);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let investigated: Vec<String> = files_touched.iter().take(20).cloned().collect();

    let mut learned: Vec<String> = Vec::new();
    for e in errors_resolved.iter().take(3) {
        learned.push(e.chars().take(80).collect());
    }
    for g in &git_events {
        if learned.len() >= 5 {
            break;
        }
        let sha: String = g
            .trim()
            .chars()
            .take(7)
            .filter(|c| c.is_ascii_hexdigit())
            .collect();
        if sha.len() == 7 {
            learned.push(format!("git:{}", sha));
        }
    }

    for e in &errors_resolved {
        if next_steps.len() >= 5 {
            break;
        }
        let entry: String = e.chars().take(80).collect();
        if !next_steps.contains(&entry) {
            next_steps.push(entry);
        }
    }
    completed.truncate(5);
    next_steps.truncate(5);

    // Which files were committed (not in `git status --porcelain`)
    let dirty = git(&["status", "--porcelain"]);
    let files_committed: Vec<String> = files_touched
        .iter()
        // git status --porcelain lines end with the path, so use ends_with
        // to avoid false positives from substring matches (e.g. "foo.rs" in "foo.rs.bak")
        .filter(|f| !dirty.lines().any(|l| l.ends_with(f.as_str())))
        .cloned()
        .collect();

    let now = session::unix_now();
    let duration_min = if prev.start_ts > 0 {
        now.saturating_sub(prev.start_ts) / 60
    } else {
        0
    };

    // ── Token economy: compute efficiency score ────────────────────────
    let ctx = crate::context::cache::SessionContext::load(sessions_dir);
    let budget = config.compact_threshold_tokens * 5 / 4;
    let total_tokens = ctx.tokens_bash + ctx.tokens_read + ctx.tokens_other;
    let dedup_hits = ctx.exact_dedup_hits + ctx.fuzzy_dedup_hits;
    let eff = crate::economy::efficiency::compute(
        total_in,
        total_out,
        ctx.agent_estimated_tokens,
        total_tokens,
        dedup_hits,
        ctx.call_counter,
        budget,
    );

    memory::write_summary(
        memory_dir,
        &memory::Summary {
            date: session::unix_to_date(prev.start_ts),
            duration_min,
            tokens_saved: total_in.saturating_sub(total_out),
            files_touched,
            files_committed,
            test_summary,
            errors_resolved,
            git_events,
            ts: prev.start_ts,
            valid_from: prev.start_ts,
            valid_to: 0,
            investigated,
            learned,
            completed,
            next_steps,
            compression_ratio_bp: eff.compression_ratio_bp,
            tool_choice_efficiency_bp: eff.tool_choice_efficiency_bp,
            context_reuse_rate_bp: eff.context_reuse_rate_bp,
            budget_utilization_bp: eff.budget_utilization_bp,
            efficiency_overall_bp: eff.overall_bp,
        },
    );
    memory::prune_old(memory_dir, config.memory_retention_days);
}

/// Check ~/.claude/CLAUDE.md size and warn if > 4096 bytes (~1K tokens).
/// Research recommendation: keep CLAUDE.md < 1K tokens — it is re-read every turn.
fn check_claude_md_size() {
    let home = crate::session::home_dir();
    let path = format!("{}/.claude/CLAUDE.md", home);
    if let Ok(meta) = std::fs::metadata(&path) {
        let bytes = meta.len();
        if bytes > 4096 {
            println!(
                "[squeez: CLAUDE.md is {}k bytes — consider trimming to <1K tokens to reduce per-turn cost]",
                bytes / 1024
            );
        }
    }
}

fn git(args: &[&str]) -> String {
    std::process::Command::new("git")
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

fn dedup_extend(dest: &mut Vec<String>, src: Vec<String>) {
    for item in src {
        if !dest.contains(&item) {
            dest.push(item);
        }
    }
}

