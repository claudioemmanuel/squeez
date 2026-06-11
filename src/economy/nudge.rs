// Auto-curation nudges (item 1 from Hermes-inspired roadmap).
//
// When recurring patterns are detected within a session, emit a single
// `[squeez: hint ...]` marker so the LLM is prompted to persist a learning,
// commit, or batch work. Each pattern key fires at most once per session.
//
// Counters live on `SessionContext` (persisted in context.json) so they
// survive across the per-call sub-process boundary.

use crate::config::Config;
use crate::context::cache::{FileAccess, SessionContext};
use crate::context::hash::fnv1a_64;

/// Evaluate the current call against the session's recurrence counters and
/// return zero-or-more nudge hint lines. Mutates `ctx` to bump counters and
/// record which keys have already been emitted (so the same hint never repeats).
pub fn evaluate(
    ctx: &mut SessionContext,
    cmd: &str,
    files: &[String],
    file_access: FileAccess,
    errors: &[String],
    cfg: &Config,
) -> Vec<String> {
    if !cfg.nudge_enabled {
        return Vec::new();
    }

    let mut hints: Vec<String> = Vec::new();

    // Recurring errors: bump every observation, nudge at threshold.
    for e in errors {
        let normalized = crate::context::cache::normalize_error(e);
        let fp = fnv1a_64(normalized.as_bytes());
        let count = ctx.bump_error_count(fp);
        if count >= cfg.nudge_error_threshold {
            let key = format!("err:{:016x}", fp);
            if ctx.mark_nudged(&key) {
                let snippet: String = e.trim().chars().take(80).collect();
                hints.push(format!(
                    "[squeez: hint — error seen ×{}: \"{}\" — consider documenting the fix in CLAUDE.md/AGENTS.md]",
                    count, snippet
                ));
            }
        }
    }

    // Repeatedly-modified files. Only bump on writes/creates/deletes — reading
    // a file 5 times isn't a curation signal, but editing it 5x without
    // committing is.
    let counts_write = matches!(
        file_access,
        FileAccess::Write | FileAccess::Created | FileAccess::Deleted
    );
    if counts_write {
        for p in files {
            let count = ctx.bump_file_mod(p);
            if count >= cfg.nudge_file_mod_threshold {
                let key = format!("file:{}", p);
                if ctx.mark_nudged(&key) {
                    hints.push(format!(
                        "[squeez: hint — {} modified ×{} without commit — consider `git add` + `git commit`]",
                        p, count
                    ));
                }
            }
        }
    }

    // Repeated expensive commands. We treat the first whitespace token as the
    // command name (matches the convention used in wrap.rs for the bash header).
    if let Some(cmd_name) = first_token(cmd) {
        if is_expensive(cmd_name) {
            let count = ctx.bump_cmd_repeat(cmd_name);
            if count >= cfg.nudge_cmd_repeat_threshold {
                let key = format!("cmd:{}", cmd_name);
                if ctx.mark_nudged(&key) {
                    hints.push(format!(
                        "[squeez: hint — `{}` run ×{} — consider scripting or caching the result]",
                        cmd_name, count
                    ));
                }
            }
            // G3: Loop pattern — when the same command is run at 2× the repeat
            // threshold, the pattern shifts from "inefficiency" to "possible loop".
            let loop_threshold = cfg.nudge_cmd_repeat_threshold.saturating_mul(2);
            if count >= loop_threshold {
                let key = format!("loop:{}", cmd_name);
                if ctx.mark_nudged(&key) {
                    hints.push(format!(
                        "[squeez: LOOP PATTERN — `{}` run ×{} — check for agent loop; \
                         if polling, replace with /monitor]",
                        cmd_name, count
                    ));
                }
            }
        }
    }

    // G4: /loop sleep-based polling nudge — if the command contains `sleep`
    // and has been repeated, it's likely a busy-poll; suggest /monitor.
    if cmd.contains("sleep") {
        let count = ctx.bump_cmd_repeat("__sleep_poll__");
        if count >= 2 {
            let key = "sleep_poll_nudge";
            if ctx.mark_nudged(key) {
                hints.push(
                    "[squeez: POLLING DETECTED — sleep-based loop burns tokens during idle waits; \
                     use /monitor to watch files/logs w/o token cost]"
                        .to_string(),
                );
            }
        }
    }

    hints
}

/// Detect quota/plan-limit error lines in tool output, bump the recurrence
/// counter, and queue a stop-retrying warning once `quota_error_threshold`
/// is reached (transcript audit CF-3). Unlike the generic error nudge above,
/// this scans raw content — quota errors rarely carry an `error:` prefix
/// (e.g. Figma's "You've reached the tool call limit on the Starter plan") —
/// and fires at a lower threshold because the error class is never transient.
/// The warning is queued on `ctx` and printed by the next `squeez wrap`.
/// Fires at most once per error fingerprint per session.
pub fn note_quota_errors(
    ctx: &mut SessionContext,
    tool: &str,
    content: &str,
    cfg: &Config,
) {
    if cfg.quota_error_threshold == 0 {
        return;
    }
    let Some(line) = content
        .lines()
        .find(|l| crate::context::cache::is_quota_error(l))
    else {
        return;
    };
    let normalized = crate::context::cache::normalize_error(line);
    let fp = fnv1a_64(normalized.as_bytes());
    let count = ctx.bump_error_count(fp);
    if count >= cfg.quota_error_threshold {
        let key = format!("quota:{:016x}", fp);
        if ctx.mark_nudged(&key) {
            let snippet: String = line.trim().chars().take(80).collect();
            ctx.queue_warning(&format!(
                "quota/plan-limit error seen ×{} via {}: \"{}\" — this error is not \
                 transient; stop retrying this tool and switch strategy",
                count, tool, snippet
            ));
        }
    }
}

/// Strip path prefix and return the first whitespace-separated token of `cmd`.
/// `/usr/local/bin/cargo test` → `cargo`. Returns None for empty input.
fn first_token(cmd: &str) -> Option<&str> {
    let first = cmd.split_whitespace().next()?;
    Some(first.rsplit('/').next().unwrap_or(first))
}

/// Commands whose repeated invocation suggests automation or a cached build.
/// Conservative list: long-running builds, full test suites, and full repo
/// scans. Quick read-only commands (`ls`, `cat`, `git status`) are excluded.
fn is_expensive(name: &str) -> bool {
    matches!(
        name,
        "cargo"
            | "make"
            | "cmake"
            | "gradle"
            | "mvn"
            | "xcodebuild"
            | "npm"
            | "pnpm"
            | "yarn"
            | "bun"
            | "jest"
            | "vitest"
            | "pytest"
            | "playwright"
            | "tsc"
            | "eslint"
            | "biome"
            | "next"
            | "terraform"
            | "tofu"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> SessionContext {
        SessionContext::default()
    }

    fn cfg() -> Config {
        Config::default()
    }

    #[test]
    fn no_nudge_below_threshold() {
        let mut c = ctx();
        let cfg = cfg();
        // 2 occurrences of same error: below default threshold (3) → no hint.
        let hints1 = evaluate(&mut c, "cargo build", &[], FileAccess::Read, &["error: foo".into()], &cfg);
        let hints2 = evaluate(&mut c, "cargo build", &[], FileAccess::Read, &["error: foo".into()], &cfg);
        assert!(hints1.is_empty());
        assert!(hints2.is_empty());
    }

    #[test]
    fn error_threshold_emits_once() {
        let mut c = ctx();
        let cfg = cfg();
        let err = vec!["error: connection refused on tcp://10.0.0.1:5432".to_string()];
        // Default threshold = 3. Calls 1, 2: no hint. Call 3: hint. Call 4+: no repeat.
        evaluate(&mut c, "cmd", &[], FileAccess::Read, &err, &cfg);
        evaluate(&mut c, "cmd", &[], FileAccess::Read, &err, &cfg);
        let h3 = evaluate(&mut c, "cmd", &[], FileAccess::Read, &err, &cfg);
        let h4 = evaluate(&mut c, "cmd", &[], FileAccess::Read, &err, &cfg);
        assert_eq!(h3.len(), 1);
        assert!(h3[0].contains("seen ×3"));
        assert!(h4.is_empty());
    }

    #[test]
    fn file_mod_threshold_counts_writes_only() {
        let mut c = ctx();
        let cfg = cfg();
        let files = vec!["/foo.rs".to_string()];
        // 5 read accesses → no nudge (reads don't count).
        for _ in 0..5 {
            let h = evaluate(&mut c, "cmd", &files, FileAccess::Read, &[], &cfg);
            assert!(h.is_empty());
        }
        // 5 writes → nudge on the 5th.
        for i in 1..=5 {
            let h = evaluate(&mut c, "cmd", &files, FileAccess::Write, &[], &cfg);
            if i < 5 {
                assert!(h.is_empty(), "early nudge at i={}", i);
            } else {
                assert_eq!(h.len(), 1);
                assert!(h[0].contains("/foo.rs"));
                assert!(h[0].contains("×5"));
            }
        }
    }

    #[test]
    fn cmd_repeat_threshold_only_for_expensive_commands() {
        let mut c = ctx();
        let cfg = cfg();
        // 10 invocations of `ls` (cheap) → never nudges.
        for _ in 0..10 {
            let h = evaluate(&mut c, "ls -la", &[], FileAccess::Read, &[], &cfg);
            assert!(h.is_empty());
        }
        // 4 invocations of `cargo` (expensive, threshold 4) → nudge on 4th.
        for i in 1..=4 {
            let h = evaluate(&mut c, "cargo test", &[], FileAccess::Read, &[], &cfg);
            if i < 4 {
                assert!(h.is_empty());
            } else {
                assert_eq!(h.len(), 1);
                assert!(h[0].contains("cargo"));
            }
        }
    }

    #[test]
    fn nudge_disabled_short_circuits() {
        let mut c = ctx();
        let mut cfg = cfg();
        cfg.nudge_enabled = false;
        for _ in 0..10 {
            let h = evaluate(
                &mut c,
                "cargo build",
                &["/x.rs".into()],
                FileAccess::Write,
                &["error: boom".into()],
                &cfg,
            );
            assert!(h.is_empty());
        }
    }

    #[test]
    fn path_prefix_stripped_in_cmd_name() {
        // `/usr/local/bin/cargo test` → counts under `cargo`, not full path.
        assert_eq!(first_token("/usr/local/bin/cargo test"), Some("cargo"));
        assert_eq!(first_token("cargo"), Some("cargo"));
        assert_eq!(first_token(""), None);
    }

    #[test]
    fn quota_error_warns_at_threshold_once() {
        let mut c = ctx();
        let cfg = cfg(); // quota_error_threshold = 2
        let content = "You've reached the Figma MCP tool call limit on the Starter plan.";
        // 1st sighting: counted, no warning.
        note_quota_errors(&mut c, "mcp__figma__get_screenshot", content, &cfg);
        assert!(c.pending_warnings.is_empty());
        // 2nd sighting: warning queued.
        note_quota_errors(&mut c, "mcp__figma__get_screenshot", content, &cfg);
        assert_eq!(c.pending_warnings.len(), 1);
        assert!(c.pending_warnings[0].contains("stop retrying"));
        assert!(c.pending_warnings[0].contains("×2"));
        // 3rd+: never repeats.
        note_quota_errors(&mut c, "mcp__figma__get_screenshot", content, &cfg);
        assert_eq!(c.pending_warnings.len(), 1);
    }

    #[test]
    fn quota_warning_collapses_variant_messages() {
        // Same limit error with different digits/paths → one fingerprint.
        let mut c = ctx();
        let cfg = cfg();
        note_quota_errors(&mut c, "Bash", "rate limit: retry after 30s", &cfg);
        note_quota_errors(&mut c, "Bash", "rate limit: retry after 60s", &cfg);
        assert_eq!(c.pending_warnings.len(), 1);
    }

    #[test]
    fn non_quota_content_never_warns() {
        let mut c = ctx();
        let cfg = cfg();
        for _ in 0..5 {
            note_quota_errors(&mut c, "Bash", "error: type mismatch in main.rs", &cfg);
        }
        assert!(c.pending_warnings.is_empty());
    }

    #[test]
    fn quota_disabled_via_zero_threshold() {
        let mut c = ctx();
        let mut cfg = cfg();
        cfg.quota_error_threshold = 0;
        for _ in 0..5 {
            note_quota_errors(&mut c, "Bash", "rate limit exceeded", &cfg);
        }
        assert!(c.pending_warnings.is_empty());
    }

    #[test]
    fn error_fingerprint_collapses_paths_and_digits() {
        // Two errors that differ only in path and digits should bump the same
        // counter — caller observes ×2, not two ×1 entries.
        let mut c = ctx();
        let cfg = cfg();
        evaluate(
            &mut c,
            "cmd",
            &[],
            FileAccess::Read,
            &["error: file /tmp/a.txt line 42 failed".into()],
            &cfg,
        );
        evaluate(
            &mut c,
            "cmd",
            &[],
            FileAccess::Read,
            &["error: file /var/log/b.txt line 99 failed".into()],
            &cfg,
        );
        // Both bumped the same fingerprint → one entry, count = 2.
        assert_eq!(c.error_count_fp.len(), 1);
        assert_eq!(c.error_count_n[0], 2);
    }
}
