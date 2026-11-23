//! `squeez setup` — register squeez into every detected host CLI.
//!
//! Iterates the HostAdapter registry, probes `is_installed()` for each, and
//! calls `install(bin_path)` on the ones present. Prints a per-host status
//! report. A `--host=<slug>` flag narrows to a single host.

use std::path::PathBuf;

use crate::hosts::{all_hosts, find, HostAdapter};

const DEFAULT_CONFIG_INI: &str = "\
# squeez configuration — edit to customize\n\
# https://github.com/claudioemmanuel/squeez\n\
\n\
enabled = true\n\
# Header: always | net (only when compression nets a win) | off\n\
show_header = net\n\
\n\
# Persona: off | lite | full | ultra\n\
# full  = caveman mode (~75% token cut, drop articles, fragments OK)\n\
# ultra = max compression + abbreviation substitutions (default)\n\
persona = ultra\n\
\n\
# Compression limits\n\
max_lines = 120\n\
git_log_max_commits = 20\n\
git_diff_max_lines = 150\n\
docker_logs_max_lines = 100\n\
find_max_results = 50\n\
\n\
# Context engine\n\
adaptive_intensity = true\n\
context_cache_enabled = true\n\
redundancy_cache_enabled = true\n\
summarize_threshold_lines = 300\n\
dedup_min = 2\n\
read_max_lines = 300\n\
grep_max_results = 100\n\
agent_prompt_max_tokens = 2000\n\
\n\
# Memory\n\
memory_retention_days = 30\n\
auto_compress_md = true\n\
lang = en\n\
\n\
# Pato-buddy companion (XP grows with squeez net savings, 25k tokens = 1 XP)\n\
buddy = true\n\
";

fn detect_lang(home: &str) -> &'static str {
    let claude_md = format!("{}/.claude/CLAUDE.md", home);
    if let Ok(content) = std::fs::read_to_string(&claude_md) {
        let lower = content.to_lowercase();
        if lower.contains("pt-br")
            || lower.contains("pt_br")
            || lower.contains("português")
            || lower.contains("portugues")
        {
            return "pt-BR";
        }
    }
    for var in &["LANG", "LC_ALL", "LANGUAGE"] {
        if let Ok(val) = std::env::var(var) {
            let lower = val.to_lowercase();
            if lower.starts_with("pt_br") || lower.starts_with("pt-br") {
                return "pt-BR";
            }
            if lower.starts_with("pt") {
                return "pt-BR";
            }
        }
    }
    "en"
}

fn write_default_config(data_dir: &std::path::Path, home: &str) -> std::io::Result<bool> {
    std::fs::create_dir_all(data_dir)?;
    let config_path = data_dir.join("config.ini");
    if config_path.exists() {
        return Ok(false);
    }
    let lang = detect_lang(home);
    let content = DEFAULT_CONFIG_INI.replace("lang = en", &format!("lang = {}", lang));
    std::fs::write(&config_path, content)?;
    Ok(true)
}

fn install_one(adapter: &dyn HostAdapter, bin_path: &std::path::Path) -> Result<String, String> {
    let home = crate::session::home_dir();
    adapter.migrate_data_dir();
    let data = adapter.data_dir();
    let created = write_default_config(&data, &home)
        .map_err(|e| format!("config.ini: {e}"))?;
    adapter
        .install(bin_path)
        .map_err(|e| format!("install: {e}"))?;
    Ok(if created {
        format!("installed (new config.ini)")
    } else {
        format!("installed (config.ini preserved)")
    })
}

/// Hosts named by `--host=a,b` (or `--hosts=`), else by `SQUEEZ_HOSTS`, else
/// `None` = every detected host. `build.sh` runs `setup`, so `SQUEEZ_HOSTS`
/// is how a single-host user stops every rebuild re-registering the rest.
pub fn host_selection(args: &[String], env: Option<String>) -> Option<Vec<String>> {
    let flag = args.iter().rev().find_map(|a| {
        a.strip_prefix("--host=")
            .or_else(|| a.strip_prefix("--hosts="))
            .map(str::to_string)
    });
    let list: Vec<String> = flag
        .or(env)?
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    (!list.is_empty()).then_some(list)
}

pub fn run(args: &[String]) -> i32 {
    let selection = host_selection(args, std::env::var("SQUEEZ_HOSTS").ok());

    let bin_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("squeez"));

    let targets: Vec<Box<dyn HostAdapter>> = match &selection {
        Some(slugs) => {
            let mut out = Vec::new();
            for slug in slugs {
                match find(slug) {
                    Some(a) => out.push(a),
                    None => {
                        eprintln!("squeez setup: unknown host '{}'", slug);
                        let names: Vec<&str> = all_hosts().iter().map(|h| h.name()).collect();
                        eprintln!("available: {}", names.join(", "));
                        return 1;
                    }
                }
            }
            out
        }
        None => all_hosts(),
    };

    let mut any_installed = false;
    let mut failures = 0;

    for adapter in targets {
        let name = adapter.name();
        if !adapter.is_installed() {
            println!("squeez setup: {}  ⏭ skipped (host not detected)", name);
            continue;
        }
        any_installed = true;
        match install_one(adapter.as_ref(), &bin_path) {
            Ok(msg) => println!("squeez setup: {}  ✓ {}", name, msg),
            Err(e) => {
                eprintln!("squeez setup: {}  ✗ {}", name, e);
                failures += 1;
            }
        }
    }

    if !any_installed {
        eprintln!(
            "squeez setup: no supported host detected on disk (looked for ~/.claude, ~/.copilot, ~/.config/opencode, ~/.gemini, ~/.codex)"
        );
        return 1;
    }

    if failures > 0 {
        return 1;
    }

    let version = crate::commands::update::current_version();
    println!("squeez setup: done — squeez {} installed. Restart the host CLI to activate.", version);
    0
}

pub fn run_with_help(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return 0;
    }
    run(args)
}

fn print_help() {
    println!("squeez setup — register squeez into every detected host CLI");
    println!();
    println!("Usage:");
    println!("  squeez setup                 Install into every detected host");
    println!("  squeez setup --host=<a,b>    Install into the listed hosts only");
    println!("  SQUEEZ_HOSTS=<a,b>           Same, from the environment (build.sh honours it)");
    println!();
    println!("Supported hosts: claude-code, copilot, opencode, gemini, codex, pi, hermes");
    println!("Extras are opt-out in config.ini: persona = off, buddy = false, auto_compress_md = false");
}

#[cfg(test)]
mod host_selection_tests {
    use super::host_selection;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn flag_list_env_and_default() {
        assert_eq!(host_selection(&args(&[]), None), None);
        assert_eq!(
            host_selection(&args(&["--host=claude-code"]), None),
            Some(vec!["claude-code".to_string()])
        );
        assert_eq!(
            host_selection(&args(&["--hosts=claude-code, codex"]), None),
            Some(vec!["claude-code".to_string(), "codex".to_string()])
        );
        assert_eq!(
            host_selection(&args(&[]), Some("opencode".into())),
            Some(vec!["opencode".to_string()])
        );
        // The flag wins over the environment.
        assert_eq!(
            host_selection(&args(&["--host=pi"]), Some("opencode".into())),
            Some(vec!["pi".to_string()])
        );
        assert_eq!(host_selection(&args(&[]), Some(" , ".into())), None);
    }
}
