//! `squeez config` — the single safe path to read/write `config.ini`.
//!
//! A slash command / skill cannot mutate the ini safely on its own (sed-editing
//! a config from a prompt is fragile and silently corrupts the file), so all
//! reads and writes go through this subcommand. Validation reuses the same
//! key/type schema `Config::from_str` understands, so unknown keys and
//! type-invalid values are rejected instead of being written as garbage.
//!
//! Subcommands:
//!   squeez config path                 — print the config.ini path
//!   squeez config get <key>            — print one key's current value
//!   squeez config list [--json]        — every key: value, default, changed?
//!   squeez config set <key> <value>    — validate + write (preserves comments)
//!   squeez config reset <key> | --all  — drop the key line(s) → back to default

use crate::config::Config;
use std::path::{Path, PathBuf};

/// The type of a config key. The canonical key list (`SCHEMA`) pairs every
/// key with its kind; `validate` parses against it so writes can never store
/// a value `Config::from_str` would later silently drop.
#[derive(Copy, Clone, PartialEq)]
enum Kind {
    Bool,
    Usize,
    U32,
    U64,
    F32,
    Persona,
    Summary,
    Lang,
    List,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Bool => "bool (true|false)",
            Kind::Usize | Kind::U32 | Kind::U64 => "integer",
            Kind::F32 => "float",
            Kind::Persona => "off|lite|full|ultra",
            Kind::Summary => "prose|structured|auto",
            Kind::Lang => "en|pt-BR",
            Kind::List => "comma-separated list",
        }
    }
}

/// Canonical schema — every key `Config::from_str` reads, with its kind.
/// Keep in sync with `Config` (a test asserts `field_value` covers every key).
const SCHEMA: &[(&str, Kind)] = &[
    ("enabled", Kind::Bool),
    ("show_header", Kind::Bool),
    ("max_lines", Kind::Usize),
    ("dedup_min", Kind::Usize),
    ("git_log_max_commits", Kind::Usize),
    ("git_diff_max_lines", Kind::Usize),
    ("docker_logs_max_lines", Kind::Usize),
    ("find_max_results", Kind::Usize),
    ("bypass", Kind::List),
    ("compact_threshold_tokens", Kind::U64),
    ("memory_retention_days", Kind::U32),
    ("adaptive_intensity", Kind::Bool),
    ("context_cache_enabled", Kind::Bool),
    ("redundancy_cache_enabled", Kind::Bool),
    ("summarize_threshold_lines", Kind::Usize),
    ("persona", Kind::Persona),
    ("auto_compress_md", Kind::Bool),
    ("lang", Kind::Lang),
    ("agent_warn_threshold_pct", Kind::F32),
    ("burn_rate_warn_calls", Kind::U64),
    ("agent_spawn_cost", Kind::U64),
    ("read_max_lines", Kind::Usize),
    ("grep_max_results", Kind::Usize),
    ("max_call_log", Kind::Usize),
    ("recent_window", Kind::U64),
    ("similarity_threshold", Kind::F32),
    ("ultra_trigger_pct", Kind::F32),
    ("mcp_prior_summaries_default", Kind::Usize),
    ("mcp_recent_calls_default", Kind::Usize),
    ("state_warn_calls", Kind::U64),
    ("sig_mode_enabled", Kind::Bool),
    ("sig_mode_threshold_lines", Kind::Usize),
    ("sig_mode_include_indented", Kind::Bool),
    ("memory_file_warn_tokens", Kind::Usize),
    ("summary_format", Kind::Summary),
    ("agent_prompt_max_tokens", Kind::Usize),
    ("plan_mode_passthrough", Kind::Bool),
    ("nudge_enabled", Kind::Bool),
    ("nudge_error_threshold", Kind::U32),
    ("nudge_file_mod_threshold", Kind::U32),
    ("nudge_cmd_repeat_threshold", Kind::U32),
    ("handler_stats_enabled", Kind::Bool),
    ("read_summarize_threshold_lines", Kind::Usize),
    ("strip_edit_preamble", Kind::Bool),
    ("context_window_tokens", Kind::U64),
    ("subagent_result_warn_tokens", Kind::Usize),
    ("quota_error_threshold", Kind::U32),
    ("cache_idle_warn_secs", Kind::U64),
    ("parallel_agent_burst_window_secs", Kind::U64),
    ("parallel_agent_burst_threshold", Kind::Usize),
    ("subagent_total_warn_tokens", Kind::U64),
    ("call_rate_warn_per_min", Kind::U32),
    ("retrieve_enabled", Kind::Bool),
    ("retrieve_ttl_days", Kind::U64),
    ("retrieve_min_lines", Kind::Usize),
];

fn kind_of(key: &str) -> Option<Kind> {
    SCHEMA.iter().find(|(k, _)| *k == key).map(|(_, kind)| *kind)
}

/// Resolve the config.ini path the same way `Config::load` does.
fn config_path() -> PathBuf {
    let base = std::env::var("SQUEEZ_DIR")
        .unwrap_or_else(|_| format!("{}/.claude/squeez", crate::session::home_dir()));
    PathBuf::from(base).join("config.ini")
}

/// Format one key's current value off a loaded `Config` as a string.
/// Returns `None` for an unknown key. Covers every key in `SCHEMA`.
fn field_value(c: &Config, key: &str) -> Option<String> {
    let v = match key {
        "enabled" => c.enabled.to_string(),
        "show_header" => c.show_header.to_string(),
        "max_lines" => c.max_lines.to_string(),
        "dedup_min" => c.dedup_min.to_string(),
        "git_log_max_commits" => c.git_log_max_commits.to_string(),
        "git_diff_max_lines" => c.git_diff_max_lines.to_string(),
        "docker_logs_max_lines" => c.docker_logs_max_lines.to_string(),
        "find_max_results" => c.find_max_results.to_string(),
        "bypass" => c.bypass.join(","),
        "compact_threshold_tokens" => c.compact_threshold_tokens.to_string(),
        "memory_retention_days" => c.memory_retention_days.to_string(),
        "adaptive_intensity" => c.adaptive_intensity.to_string(),
        "context_cache_enabled" => c.context_cache_enabled.to_string(),
        "redundancy_cache_enabled" => c.redundancy_cache_enabled.to_string(),
        "summarize_threshold_lines" => c.summarize_threshold_lines.to_string(),
        "persona" => crate::commands::persona::as_str(c.persona).to_string(),
        "auto_compress_md" => c.auto_compress_md.to_string(),
        "lang" => c.lang.clone(),
        "agent_warn_threshold_pct" => c.agent_warn_threshold_pct.to_string(),
        "burn_rate_warn_calls" => c.burn_rate_warn_calls.to_string(),
        "agent_spawn_cost" => c.agent_spawn_cost.to_string(),
        "read_max_lines" => c.read_max_lines.to_string(),
        "grep_max_results" => c.grep_max_results.to_string(),
        "max_call_log" => c.max_call_log.to_string(),
        "recent_window" => c.recent_window.to_string(),
        "similarity_threshold" => c.similarity_threshold.to_string(),
        "ultra_trigger_pct" => c.ultra_trigger_pct.to_string(),
        "mcp_prior_summaries_default" => c.mcp_prior_summaries_default.to_string(),
        "mcp_recent_calls_default" => c.mcp_recent_calls_default.to_string(),
        "state_warn_calls" => c.state_warn_calls.to_string(),
        "sig_mode_enabled" => c.sig_mode_enabled.to_string(),
        "sig_mode_threshold_lines" => c.sig_mode_threshold_lines.to_string(),
        "sig_mode_include_indented" => c.sig_mode_include_indented.to_string(),
        "memory_file_warn_tokens" => c.memory_file_warn_tokens.to_string(),
        "summary_format" => c.summary_format.clone(),
        "agent_prompt_max_tokens" => c.agent_prompt_max_tokens.to_string(),
        "plan_mode_passthrough" => c.plan_mode_passthrough.to_string(),
        "nudge_enabled" => c.nudge_enabled.to_string(),
        "nudge_error_threshold" => c.nudge_error_threshold.to_string(),
        "nudge_file_mod_threshold" => c.nudge_file_mod_threshold.to_string(),
        "nudge_cmd_repeat_threshold" => c.nudge_cmd_repeat_threshold.to_string(),
        "handler_stats_enabled" => c.handler_stats_enabled.to_string(),
        "read_summarize_threshold_lines" => c.read_summarize_threshold_lines.to_string(),
        "strip_edit_preamble" => c.strip_edit_preamble.to_string(),
        "context_window_tokens" => c.context_window_tokens.to_string(),
        "subagent_result_warn_tokens" => c.subagent_result_warn_tokens.to_string(),
        "quota_error_threshold" => c.quota_error_threshold.to_string(),
        "cache_idle_warn_secs" => c.cache_idle_warn_secs.to_string(),
        "parallel_agent_burst_window_secs" => c.parallel_agent_burst_window_secs.to_string(),
        "parallel_agent_burst_threshold" => c.parallel_agent_burst_threshold.to_string(),
        "subagent_total_warn_tokens" => c.subagent_total_warn_tokens.to_string(),
        "call_rate_warn_per_min" => c.call_rate_warn_per_min.to_string(),
        "retrieve_enabled" => c.retrieve_enabled.to_string(),
        "retrieve_ttl_days" => c.retrieve_ttl_days.to_string(),
        "retrieve_min_lines" => c.retrieve_min_lines.to_string(),
        _ => return None,
    };
    Some(v)
}

/// Validate a `key = value` pair against the schema. Returns the normalized
/// value to write on success, or a human-readable error.
fn validate(key: &str, value: &str) -> Result<String, String> {
    let kind = kind_of(key).ok_or_else(|| {
        format!(
            "unknown key '{key}'. Run `squeez config list` to see all valid keys."
        )
    })?;
    let v = value.trim();
    let ok = match kind {
        Kind::Bool => matches!(v, "true" | "false"),
        Kind::Usize => v.parse::<usize>().is_ok(),
        Kind::U32 => v.parse::<u32>().is_ok(),
        Kind::U64 => v.parse::<u64>().is_ok(),
        Kind::F32 => v.parse::<f32>().is_ok(),
        Kind::Persona => matches!(v.to_lowercase().as_str(), "off" | "lite" | "full" | "ultra"),
        Kind::Summary => matches!(v, "prose" | "structured" | "auto"),
        Kind::Lang => matches!(v.to_lowercase().as_str(), "en" | "pt-br"),
        Kind::List => !v.is_empty(),
    };
    if !ok {
        return Err(format!(
            "invalid value '{value}' for '{key}' (expected {})",
            kind.label()
        ));
    }
    // Normalize a couple of kinds so the written form matches what the loader
    // and the rest of the codebase expect.
    let normalized = match kind {
        Kind::Persona => v.to_lowercase(),
        Kind::Lang => {
            if v.eq_ignore_ascii_case("pt-br") {
                "pt-BR".to_string()
            } else {
                "en".to_string()
            }
        }
        _ => v.to_string(),
    };
    Ok(normalized)
}

/// True if `line` is a non-comment assignment of `key`.
fn assigns_key(line: &str, key: &str) -> bool {
    let t = line.trim_start();
    if t.starts_with('#') {
        return false;
    }
    match t.split_once('=') {
        Some((k, _)) => k.trim() == key,
        None => false,
    }
}

/// Read config.ini (empty if absent).
fn read_ini(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Atomically write `contents` to `path`, creating parent dirs.
fn write_ini(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("ini.tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}

/// Set `key = value`, preserving every comment and unknown line. Replaces an
/// existing uncommented assignment in place; otherwise appends.
fn write_set(path: &Path, key: &str, value: &str) -> std::io::Result<()> {
    let existing = read_ini(path);
    let mut out: Vec<String> = Vec::new();
    let mut replaced = false;
    for line in existing.lines() {
        if !replaced && assigns_key(line, key) {
            out.push(format!("{key} = {value}"));
            replaced = true;
        } else {
            out.push(line.to_string());
        }
    }
    if !replaced {
        if !out.is_empty() && !out.last().map(|l| l.is_empty()).unwrap_or(true) {
            // keep a tidy file; no extra blank needed
        }
        out.push(format!("{key} = {value}"));
    }
    let mut contents = out.join("\n");
    contents.push('\n');
    write_ini(path, &contents)
}

/// Remove uncommented assignments of every key in `keys`, preserving the rest.
fn write_reset(path: &Path, keys: &[&str]) -> std::io::Result<()> {
    let existing = read_ini(path);
    let kept: Vec<&str> = existing
        .lines()
        .filter(|line| !keys.iter().any(|k| assigns_key(line, k)))
        .collect();
    let mut contents = kept.join("\n");
    if !contents.is_empty() {
        contents.push('\n');
    }
    write_ini(path, &contents)
}

fn load_from(path: &Path) -> Config {
    Config::from_str(&read_ini(path))
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

pub fn run(args: &[String]) -> i32 {
    let path = config_path();
    match args.first().map(String::as_str) {
        Some("path") => {
            println!("{}", path.display());
            0
        }
        Some("get") => {
            let Some(key) = args.get(1) else {
                eprintln!("squeez config get: missing <key>");
                return 1;
            };
            let cfg = load_from(&path);
            match field_value(&cfg, key) {
                Some(v) => {
                    println!("{v}");
                    0
                }
                None => {
                    eprintln!(
                        "squeez config get: unknown key '{key}'. Run `squeez config list`."
                    );
                    1
                }
            }
        }
        Some("set") => {
            let (Some(key), Some(value)) = (args.get(1), args.get(2)) else {
                eprintln!("squeez config set: usage `squeez config set <key> <value>`");
                return 1;
            };
            // Allow values with spaces (e.g. a bypass list) to be passed as
            // separate argv items.
            let value = if args.len() > 3 {
                args[2..].join(" ")
            } else {
                value.to_string()
            };
            match validate(key, &value) {
                Ok(normalized) => {
                    if let Err(e) = write_set(&path, key, &normalized) {
                        eprintln!("squeez config set: write failed: {e}");
                        return 1;
                    }
                    println!("{key} = {normalized}");
                    if key == "persona" || key == "lang" {
                        println!(
                            "note: {key} is injected at SessionStart — run `squeez init` to \
                             rewrite the CLAUDE.md block now; it fully lands on the next \
                             session / after /compact."
                        );
                    }
                    0
                }
                Err(e) => {
                    eprintln!("squeez config set: {e}");
                    1
                }
            }
        }
        Some("reset") => {
            let cur = load_from(&path);
            match args.get(1).map(String::as_str) {
                Some("--all") => {
                    let keys: Vec<&str> = SCHEMA.iter().map(|(k, _)| *k).collect();
                    if let Err(e) = write_reset(&path, &keys) {
                        eprintln!("squeez config reset: write failed: {e}");
                        return 1;
                    }
                    println!("reset all keys to defaults");
                    let _ = cur;
                    0
                }
                Some(key) => {
                    if kind_of(key).is_none() {
                        eprintln!(
                            "squeez config reset: unknown key '{key}'. Run `squeez config list`."
                        );
                        return 1;
                    }
                    if let Err(e) = write_reset(&path, &[key]) {
                        eprintln!("squeez config reset: write failed: {e}");
                        return 1;
                    }
                    let def = field_value(&Config::default(), key).unwrap_or_default();
                    println!("{key} reset to default ({def})");
                    0
                }
                None => {
                    eprintln!("squeez config reset: usage `squeez config reset <key> | --all`");
                    1
                }
            }
        }
        Some("list") => {
            let json = args.iter().any(|a| a == "--json");
            let cur = load_from(&path);
            let def = Config::default();
            if json {
                let mut items: Vec<String> = Vec::new();
                for (key, _) in SCHEMA {
                    let value = field_value(&cur, key).unwrap_or_default();
                    let default = field_value(&def, key).unwrap_or_default();
                    let changed = value != default;
                    items.push(format!(
                        "{{\"key\":\"{}\",\"value\":\"{}\",\"default\":\"{}\",\"changed\":{}}}",
                        json_escape(key),
                        json_escape(&value),
                        json_escape(&default),
                        changed
                    ));
                }
                println!("[{}]", items.join(","));
            } else {
                for (key, _) in SCHEMA {
                    let value = field_value(&cur, key).unwrap_or_default();
                    let default = field_value(&def, key).unwrap_or_default();
                    if value != default {
                        println!("{key} = {value}  (default {default}) *");
                    } else {
                        println!("{key} = {value}");
                    }
                }
            }
            0
        }
        Some(other) => {
            eprintln!("squeez config: unknown subcommand '{other}'");
            print_help();
            1
        }
        None => {
            print_help();
            1
        }
    }
}

fn print_help() {
    eprintln!("squeez config — inspect and change squeez settings");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  squeez config path                 Print the config.ini path");
    eprintln!("  squeez config get <key>            Print one key's value");
    eprintln!("  squeez config list [--json]        All keys (changed marked *)");
    eprintln!("  squeez config set <key> <value>    Validate + write (keeps comments)");
    eprintln!("  squeez config reset <key> | --all  Drop key line(s) → default");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_covered_by_field_value() {
        let def = Config::default();
        for (key, _) in SCHEMA {
            assert!(
                field_value(&def, key).is_some(),
                "field_value missing arm for schema key '{key}'"
            );
        }
    }

    #[test]
    fn validate_rejects_unknown_and_bad_values() {
        assert!(validate("not_a_key", "x").is_err());
        assert!(validate("enabled", "yes").is_err());
        assert!(validate("max_lines", "lots").is_err());
        assert!(validate("ultra_trigger_pct", "high").is_err());
        assert!(validate("persona", "caveman").is_err());
        assert!(validate("summary_format", "fancy").is_err());
    }

    #[test]
    fn validate_accepts_and_normalizes() {
        assert_eq!(validate("enabled", "false").unwrap(), "false");
        assert_eq!(validate("max_lines", "200").unwrap(), "200");
        assert_eq!(validate("persona", "ULTRA").unwrap(), "ultra");
        assert_eq!(validate("lang", "pt-br").unwrap(), "pt-BR");
        assert_eq!(validate("ultra_trigger_pct", "0.8").unwrap(), "0.8");
    }

    #[test]
    fn set_get_round_trips_and_preserves_comments() {
        let dir = std::env::temp_dir().join(format!("sqcfg_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.ini");
        std::fs::write(&path, "# my comment\nenabled = true\nmax_lines = 120\n").unwrap();

        write_set(&path, "max_lines", &validate("max_lines", "42").unwrap()).unwrap();
        let cfg = load_from(&path);
        assert_eq!(field_value(&cfg, "max_lines").unwrap(), "42");

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("# my comment"), "comment must survive: {raw}");
        assert!(raw.contains("enabled = true"), "other keys must survive: {raw}");
        // The key was replaced in place, not duplicated.
        assert_eq!(raw.matches("max_lines =").count(), 1, "{raw}");

        // A brand-new key appends.
        write_set(&path, "persona", &validate("persona", "lite").unwrap()).unwrap();
        assert_eq!(load_from(&path).persona, crate::commands::persona::Persona::Lite);

        // Reset drops the line → back to default.
        write_reset(&path, &["max_lines"]).unwrap();
        assert_eq!(load_from(&path).max_lines, Config::default().max_lines);
        let raw2 = std::fs::read_to_string(&path).unwrap();
        assert!(raw2.contains("# my comment"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
