//! Flag-forcing: two independent, additive tiers that make a wrapped
//! command emit better-compressible output without silently changing its
//! result.
//!
//! - Tier "env" (config `flag_force = env`, the default): sets environment
//!   variables via `Command::env()` -- `LC_ALL`, `GIT_PAGER`, `NO_COLOR`,
//!   `FORCE_COLOR`. These only affect *formatting* (locale-dependent
//!   sorting, pager invocation, ANSI color) that every well-behaved CLI
//!   already supports disabling; they never change what a command does.
//! - Tier "arg" (config `flag_force = full`, opt-in): appends a
//!   machine-readable-output flag (`--json`, `-json`, `--format json`) to
//!   the command STRING itself, for the runners `commands::reporters` has a
//!   structured parser for. This DOES change the command's own output
//!   format, so it is gated hard by the caller in `wrap.rs`: never applied
//!   to a command containing shell metacharacters (the injected flag could
//!   land on the wrong stage of a pipeline/redirect/subshell), never
//!   applied twice to the same base command after one failure (see
//!   `looks_like_unrecognized_option` + `SessionContext::flag_force_failed`),
//!   and never applied to a `flag_force_deny`-listed prefix.

/// Env vars always safe to set: disable locale/pager/color quirks without
/// changing what the command does.
pub const ENV_VARS: &[(&str, &str)] = &[
    ("LC_ALL", "C"),
    ("GIT_PAGER", "cat"),
    ("NO_COLOR", "1"),
    ("FORCE_COLOR", "0"),
];

/// True when `cmd` contains a shell construct that makes it unsafe to
/// append a flag to the string -- the flag could land on the wrong command
/// in a pipeline, get redirected away, or run inside an unrelated subshell.
pub fn has_shell_metachars(cmd: &str) -> bool {
    cmd.contains('|')
        || cmd.contains('>')
        || cmd.contains('<')
        || cmd.contains("&&")
        || cmd.contains(';')
        || cmd.contains("$(")
}

pub struct ArgInjection {
    pub cmd: String,
    pub label: String,
}

/// Appends a machine-readable-output flag when `cmd` matches a known
/// test/lint runner `commands::reporters` has a structured parser for, and
/// doesn't already request one. Returns `None` when no injection applies.
/// Caller is responsible for the shell-metachar and deny-list gates.
pub fn arg_injection(cmd: &str) -> Option<ArgInjection> {
    let first = cmd.split_whitespace().next().unwrap_or("");
    let name = first.rsplit('/').next().unwrap_or(first);

    match name {
        "jest" | "vitest" => {
            if cmd.contains("--json") {
                None
            } else {
                Some(inject(cmd, "--json"))
            }
        }
        "go" => {
            if cmd.split_whitespace().any(|a| a == "test") && !cmd.contains("-json") {
                Some(inject(cmd, "-json"))
            } else {
                None
            }
        }
        "eslint" => {
            if cmd.contains("--format") || cmd.contains(" -f ") {
                None
            } else {
                Some(inject(cmd, "--format json"))
            }
        }
        "ruff" => {
            if cmd.split_whitespace().any(|a| a == "check")
                && !cmd.contains("--output-format")
                && !cmd.contains("--format")
            {
                Some(inject(cmd, "--output-format json"))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn inject(cmd: &str, flag: &str) -> ArgInjection {
    ArgInjection {
        cmd: format!("{} {}", cmd, flag),
        label: flag.to_string(),
    }
}

/// True when `text` looks like the injected flag was rejected by the tool
/// (unknown-option-style error from any of the common CLI arg parsers) --
/// the fallback re-run trigger.
pub fn looks_like_unrecognized_option(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    t.contains("unrecognized option")
        || t.contains("unrecognized arguments")
        || t.contains("unknown option")
        || t.contains("unknown argument")
        || t.contains("not a recognized")
        || t.contains("flag provided but not defined")
        || t.contains("invalid option")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_metachars_detected() {
        assert!(has_shell_metachars("cargo test | tee out.txt"));
        assert!(has_shell_metachars("jest > out.json"));
        assert!(has_shell_metachars("jest --json && echo done"));
        assert!(has_shell_metachars("echo $(date)"));
        assert!(!has_shell_metachars("jest --coverage"));
    }

    #[test]
    fn jest_gets_json_appended_once() {
        let inj = arg_injection("jest --coverage").expect("should inject");
        assert_eq!(inj.cmd, "jest --coverage --json");
        assert_eq!(inj.label, "--json");
        assert!(arg_injection("jest --json").is_none());
    }

    #[test]
    fn go_test_gets_json_flag() {
        let inj = arg_injection("go test ./...").expect("should inject");
        assert_eq!(inj.cmd, "go test ./... -json");
        assert!(arg_injection("go build ./...").is_none());
        assert!(arg_injection("go test -json ./...").is_none());
    }

    #[test]
    fn eslint_respects_explicit_format() {
        assert!(arg_injection("eslint .").is_some());
        assert!(arg_injection("eslint . --format stylish").is_none());
    }

    #[test]
    fn ruff_only_injects_for_check() {
        assert!(arg_injection("ruff check .").is_some());
        assert!(arg_injection("ruff format .").is_none());
        assert!(arg_injection("ruff check . --output-format json").is_none());
    }

    #[test]
    fn non_reporter_commands_never_injected() {
        assert!(arg_injection("cargo test").is_none());
        assert!(arg_injection("pytest").is_none());
        assert!(arg_injection("git push").is_none());
    }

    #[test]
    fn unrecognized_option_patterns_match_common_parsers() {
        assert!(looks_like_unrecognized_option("error: unrecognized arguments: --json"));
        assert!(looks_like_unrecognized_option("flag provided but not defined: -json"));
        assert!(looks_like_unrecognized_option("Unknown argument: json"));
        assert!(!looks_like_unrecognized_option("3 tests passed"));
    }
}
