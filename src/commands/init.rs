use std::path::Path;

use crate::{
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
    run_with_dirs(&sessions, &mem, &cfg)
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

    // Inject memory banner into Copilot CLI instructions file
    let summaries = memory::read_last_n(&mem, 3);
    inject_copilot_instructions(&home, &cfg, &summaries);

    code
}

fn load_config_from(base: &str) -> Config {
    let path = format!("{}/config.ini", base);
    std::fs::read_to_string(&path)
        .map(|s| Config::from_str(&s))
        .unwrap_or_default()
}

/// Replaces the squeez block (<!-- squeez:start --> … <!-- squeez:end -->)
/// in ~/.copilot/copilot-instructions.md, creating the file if absent.
fn inject_copilot_instructions(home: &str, cfg: &Config, summaries: &[memory::Summary]) {
    let path = format!("{}/.copilot/copilot-instructions.md", home);
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    let mut block = String::from("<!-- squeez:start -->\n");
    block.push_str("## squeez — session context\n");
    let budget_k = cfg.compact_threshold_tokens * 5 / 4 / 1000;
    block.push_str(&format!(
        "Context budget: ~{}K tokens | Compression: ON | Memory: ON\n",
        budget_k
    ));
    for s in summaries {
        block.push_str(&format!("- {}\n", s.display_line()));
    }
    if summaries.is_empty() {
        block.push_str("- No prior sessions recorded yet.\n");
    }
    block.push_str("<!-- squeez:end -->\n");

    // Strip previous squeez block if present
    let cleaned = if existing.contains("<!-- squeez:start -->") {
        let start = existing.find("<!-- squeez:start -->").unwrap_or(0);
        let end = existing
            .find("<!-- squeez:end -->")
            .map(|i| i + "<!-- squeez:end -->".len() + 1) // include newline
            .unwrap_or(start);
        format!("{}{}", &existing[..start], &existing[end.min(existing.len())..])
    } else {
        existing
    };

    // Prepend the fresh block
    let contents = format!("{}\n{}", block, cleaned.trim_start());
    let _ = std::fs::write(&path, contents);
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
        compact_warned: false,
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
        "Context budget: ~{}K tokens | Compression: ON | Memory: ON",
        budget_k
    );
    for s in &summaries {
        println!("{}", s.display_line());
    }
    println!("────────────────────────────────────────────────────────────");
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
                        test_summary = ts;
                    }
                }
            }
            _ => {}
        }
    }

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
        },
    );
    memory::prune_old(memory_dir, config.memory_retention_days);
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
