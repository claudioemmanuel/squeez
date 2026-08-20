//! `squeez doctor` — read-only self-health check for the compression pipeline.
//!
//! Motivated by a real outage: `enabled=false` plus a payload-key drift left
//! the whole pipeline dead for days while the session banner claimed
//! "Compression: ON". Doctor makes that state visible: hook drift vs the
//! embedded sources, hook registration, config state, and tracking freshness.

use crate::config::Config;
use crate::hosts::claude_code::hooks_manifest;
use crate::json_util::JsonValue;
use crate::session;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// One check line plus whether it is a hard failure.
struct CheckLine {
    line: String,
    fail: bool,
}

fn ok(msg: impl Into<String>) -> CheckLine {
    CheckLine { line: format!("[ok]   {}", msg.into()), fail: false }
}
fn warn(msg: impl Into<String>) -> CheckLine {
    CheckLine { line: format!("[WARN] {}", msg.into()), fail: false }
}
fn fail(msg: impl Into<String>) -> CheckLine {
    CheckLine { line: format!("[FAIL] {}", msg.into()), fail: true }
}

/// Hook-drift check: installed scripts must match the sources embedded in
/// this binary. A stale hook means the binary was updated but `squeez setup`
/// never re-ran (or someone edited the installed copy).
fn check_hooks_drift(squeez_dir: &Path) -> CheckLine {
    let mut stale: Vec<&str> = Vec::new();
    for (name, embedded) in hooks_manifest() {
        let installed = squeez_dir.join("hooks").join(name);
        match std::fs::read_to_string(&installed) {
            Ok(content) if content == embedded => {}
            _ => stale.push(name),
        }
    }
    if stale.is_empty() {
        ok("hooks: installed scripts match this binary")
    } else {
        fail(format!(
            "hooks: stale or missing ({}) — run `squeez setup`",
            stale.join(", ")
        ))
    }
}

/// Registration check: the host settings file must reference the squeez hooks.
fn check_hooks_registered(settings_path: &Path) -> CheckLine {
    let content = std::fs::read_to_string(settings_path).unwrap_or_default();
    if content.is_empty() {
        return fail(format!(
            "registration: {} missing or unreadable — run `squeez setup`",
            settings_path.display()
        ));
    }
    let missing: Vec<&str> = ["pretooluse.sh", "posttooluse.sh", "session-start.sh"]
        .into_iter()
        .filter(|h| !content.contains(h))
        .collect();
    if missing.is_empty() {
        ok("registration: hooks present in settings.json")
    } else {
        fail(format!(
            "registration: {} not registered — run `squeez setup`",
            missing.join(", ")
        ))
    }
}

/// Why a registered hook `command` cannot actually run, or `None` when it is
/// sound. Pure over `exists` so the Windows failure modes stay testable from
/// any host.
///
/// `check_hooks_registered` only asks whether the script *name* appears
/// somewhere in settings.json. That is why doctor reported all-green on an
/// install where every hook died with `No such file or directory` (#209): the
/// registered command was `bash C:\Users\Jesse\.claude\squeez\hooks\…`, bash
/// ate each backslash as an escape, and the file it then looked for did not
/// exist. Note that a plain existence check is not enough to catch this — on
/// Windows the *unmangled* path does resolve; it is the quoting that is wrong.
fn hook_command_problem(cmd: &str, exists: impl Fn(&Path) -> bool) -> Option<String> {
    // Only `bash <script>` is statically checkable. Anything else — a bare
    // path (codex/gemini invoke their hooks directly) or a compound
    // `bash -c '…'` chain (the adopted-HUD statusLine) — is left alone rather
    // than guessed at, since a false FAIL is worse than a missed one.
    let arg = cmd.strip_prefix("bash ")?.trim();
    if arg.starts_with('-') {
        return None;
    }
    let path = match arg.strip_prefix('"') {
        Some(rest) => rest.strip_suffix('"')?,
        None if arg.contains(' ') => return None,
        None if arg.contains('\\') => {
            return Some(format!(
                "unquoted backslashes — bash reads it as `{}`",
                arg.replace('\\', "")
            ))
        }
        None => arg,
    };
    if !exists(Path::new(path)) {
        return Some(format!("script not found: {path}"));
    }
    None
}

/// Every squeez hook command registered in `settings`, from both the nested
/// `hooks` map and the legacy top-level shape.
fn registered_squeez_commands(settings: &JsonValue) -> Vec<String> {
    let mut out = Vec::new();
    let push = |cmd: &str, out: &mut Vec<String>| {
        if cmd.contains("squeez") && !out.iter().any(|c| c == cmd) {
            out.push(cmd.to_string());
        }
    };
    let mut roots = vec![settings];
    if let Some(nested) = settings.get("hooks").filter(|h| h.is_obj()) {
        roots.push(nested);
    }
    for root in roots {
        for (_event, entries) in root.obj_entries() {
            for entry in entries.as_arr() {
                for hook in entry.get("hooks").map(|h| h.as_arr()).unwrap_or(&[]) {
                    push(hook.get_str("command"), &mut out);
                }
            }
        }
    }
    // statusLine is an object, not an event array, and squeez registers one too.
    if let Some(status) = settings.get("statusLine").filter(|s| s.is_obj()) {
        push(status.get_str("command"), &mut out);
    }
    out
}

/// Registered hooks must be commands a shell can actually execute — not merely
/// strings that mention the right filename.
fn check_hooks_runnable(settings_path: &Path) -> CheckLine {
    let Some(settings) = crate::hosts::settings_json::load_lenient(settings_path) else {
        return warn("runnable: settings.json unreadable — skipped");
    };
    let commands = registered_squeez_commands(&settings);
    if commands.is_empty() {
        return warn("runnable: no squeez hook commands to check");
    }
    let broken: Vec<String> = commands
        .iter()
        .filter_map(|c| hook_command_problem(c, |p| p.exists()).map(|why| format!("{c} — {why}")))
        .collect();
    if broken.is_empty() {
        return ok(format!("runnable: {} hook commands resolve", commands.len()));
    }
    fail(format!(
        "runnable: {} of {} hook commands cannot execute — run `squeez setup`\n         {}",
        broken.len(),
        commands.len(),
        broken.join("\n         ")
    ))
}

/// The hooks drive their JSON handling through Python, so a missing or broken
/// interpreter makes them no-ops. Probe by EXECUTING each candidate: on Windows
/// `python3` is usually the Microsoft Store alias stub, which is on PATH and
/// passes `command -v` but exits non-zero when run (#209).
fn check_interpreter() -> CheckLine {
    for candidate in ["python3", "python", "py"] {
        let runs = std::process::Command::new(candidate)
            .args(["-c", ""])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if runs {
            return ok(format!("interpreter: hooks will use `{candidate}`"));
        }
    }
    fail(
        "interpreter: no working python on PATH (tried python3, python, py) — \
         hooks parse their payloads with it, so compression is inert. \
         Install Python and re-run `squeez doctor`",
    )
}

/// Config check: a disabled pipeline is the exact silent-death mode doctor
/// exists to surface, so it is a FAIL, not a WARN.
fn check_config(cfg: &Config) -> CheckLine {
    let label = cfg.compression_status_label();
    if !cfg.enabled {
        fail(format!("config: compression {}", label))
    } else if !cfg.wrap_bash {
        warn(format!("config: compression {}", label))
    } else {
        ok("config: compression ON")
    }
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Newest session-log (`*.jsonl`) mtime in the sessions dir.
fn newest_session(sessions: &Path) -> Option<SystemTime> {
    let mut newest: Option<SystemTime> = None;
    for e in std::fs::read_dir(sessions).ok()?.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) == Some("jsonl") {
            if let Some(t) = mtime(&p) {
                newest = Some(newest.map_or(t, |n| n.max(t)));
            }
        }
    }
    newest
}

/// Nonzero unsigned integer value for `key` in a JSONL record line.
fn nonzero_field(line: &str, key: &str) -> bool {
    line.split(&format!("\"{}\":", key))
        .skip(1)
        .filter_map(|rest| {
            rest.trim_start()
                .split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse::<u64>()
                .ok()
        })
        .any(|v| v > 0)
}

/// True if a session-log line proves accounting is still running.
///
/// Two record shapes count: the PostToolUse `track` record (`tokens_est`) and
/// the `squeez wrap` record (`type:"bash"` with `in_tk`/`out_tk`). Bash output
/// is compressed at PreToolUse, so its PostToolUse `track` record is always
/// `tokens_est:0` — a session of purely wrapped Bash calls has live accounting
/// but no nonzero `tokens_est` anywhere.
fn accounting_alive_line(line: &str) -> bool {
    if nonzero_field(line, "tokens_est") {
        return true;
    }
    line.contains("\"type\":\"bash\"")
        && (nonzero_field(line, "in_tk") || nonzero_field(line, "out_tk"))
}

/// True if any of the newest session logs shows live token accounting —
/// distinguishes "accounting alive" from the all-zeros drift outage.
fn tokens_est_alive(sessions: &Path) -> bool {
    let mut files: Vec<(SystemTime, std::path::PathBuf)> = std::fs::read_dir(sessions)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            (p.extension().and_then(|s| s.to_str()) == Some("jsonl"))
                .then(|| mtime(&p).map(|t| (t, p)))
                .flatten()
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.iter().take(2).any(|(_, p)| {
        std::fs::read_to_string(p).is_ok_and(|s| s.lines().any(accounting_alive_line))
    })
}

/// Freshness check: sessions are running but the wrap/accounting artifacts
/// stopped moving — the "tracking dead" signature of the July outage.
fn check_freshness(squeez_dir: &Path, cfg: &Config) -> CheckLine {
    if cfg.doctor_stale_days == 0 {
        return ok("freshness: check disabled (doctor_stale_days=0)");
    }
    let sessions = squeez_dir.join("sessions");
    let Some(newest) = newest_session(&sessions) else {
        return ok("freshness: no session logs yet");
    };
    let window = Duration::from_secs(cfg.doctor_stale_days * 24 * 3600);
    let stats_stale = mtime(&sessions.join("handler_stats.json"))
        .map_or(true, |t| newest > t + window);
    if stats_stale {
        return warn(
            "freshness: sessions active but handler_stats.json is stale — \
             bash wrap not running (check `squeez should-wrap`, config, hooks)",
        );
    }
    if !tokens_est_alive(&sessions) {
        return warn(
            "freshness: recent session logs have only tokens_est:0 — \
             accounting broken (posttooluse payload extraction)",
        );
    }
    ok("freshness: tracking artifacts are current")
}

/// Blob-store check: reports current stash size (#201/#200 — this is the
/// "make it observable" half of both issues). Informational only, never a
/// FAIL/WARN — a big stash isn't unhealthy on its own, just worth seeing.
fn check_blob_store(squeez_dir: &Path) -> CheckLine {
    let (count, bytes) = crate::context::retrieve::stats_under(squeez_dir);
    ok(format!(
        "blob store: {} stashed output(s), {:.1} KB — `squeez prune` to clear expired ones early",
        count,
        bytes as f64 / 1024.0
    ))
}

/// Full doctor report. Returns the printable lines and whether any check FAILed.
pub fn run_with(squeez_dir: &Path, settings_path: &Path, cfg: &Config) -> (Vec<String>, bool) {
    let checks = [
        check_hooks_drift(squeez_dir),
        check_hooks_registered(settings_path),
        check_hooks_runnable(settings_path),
        check_interpreter(),
        check_config(cfg),
        check_freshness(squeez_dir, cfg),
        check_blob_store(squeez_dir),
    ];
    let has_fail = checks.iter().any(|c| c.fail);
    let mut lines: Vec<String> = vec![format!(
        "squeez doctor — v{} @ {}",
        env!("CARGO_PKG_VERSION"),
        squeez_dir.display()
    )];
    let mut check_lines: Vec<String> = checks.into_iter().map(|c| c.line).collect();
    if cfg.focus == crate::commands::focus::Focus::Adhd {
        // Broken first, healthy last — and one command to run at the end.
        check_lines.sort_by_key(|l| match () {
            _ if l.starts_with("[FAIL]") => 0,
            _ if l.starts_with("[WARN]") => 1,
            _ => 2,
        });
        let next = check_lines
            .iter()
            .find(|l| l.starts_with("[FAIL]") || l.starts_with("[WARN]"))
            .and_then(|l| backticked_cmd(l))
            .map(|c| format!("Next: {}", c))
            .unwrap_or_else(|| "Next: nothing to fix — pipeline healthy.".to_string());
        lines.extend(check_lines);
        lines.push(next);
    } else {
        lines.extend(check_lines);
    }
    (lines, has_fail)
}

/// First `backticked` command in a check line — the fix each failing check
/// already names, lifted out so focus mode can end with a single action.
fn backticked_cmd(line: &str) -> Option<String> {
    let start = line.find('`')? + 1;
    let rest = &line[start..];
    let end = rest.find('`')?;
    Some(rest[..end].to_string())
}

/// Cheap subset for the SessionStart banner (no session-log scan): returns a
/// single warning line when the pipeline is degraded, or None when healthy.
pub fn quick_check(squeez_dir: &Path, settings_path: &Path, cfg: &Config) -> Option<String> {
    let degraded = check_hooks_drift(squeez_dir).fail
        || check_hooks_registered(settings_path).fail
        || check_config(cfg).fail;
    degraded.then(|| "[squeez doctor: pipeline degraded — run 'squeez doctor']".to_string())
}

fn default_settings_path() -> std::path::PathBuf {
    Path::new(&session::home_dir()).join(".claude").join("settings.json")
}

/// CLI entry point: print the report, exit 1 on any FAIL.
pub fn run() -> i32 {
    let cfg = Config::load();
    let (lines, has_fail) = run_with(&session::squeez_dir(), &default_settings_path(), &cfg);
    for l in lines {
        println!("{}", l);
    }
    i32::from(has_fail)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every path in these cases is asked about through this predicate, so the
    /// Windows shapes can be exercised from a macOS CI runner.
    fn present(_p: &Path) -> bool {
        true
    }
    fn absent(_p: &Path) -> bool {
        false
    }

    /// #209: doctor was green on an install where every hook died with
    /// `No such file or directory`. On Windows the unmangled path DOES exist,
    /// so only the quoting tells the two apart.
    #[test]
    fn unquoted_backslash_command_is_reported_even_when_the_file_exists() {
        let cmd = "bash C:\\Users\\JesseKlotz\\.claude\\squeez\\hooks\\pretooluse.sh";
        let why = hook_command_problem(cmd, present).expect("must be flagged");
        assert!(why.contains("unquoted backslashes"), "{why}");
        assert!(
            why.contains("C:UsersJesseKlotz.claudesqueezhookspretooluse.sh"),
            "must show what bash actually sees: {why}"
        );
    }

    #[test]
    fn quoted_forward_slash_command_is_accepted() {
        let cmd = "bash \"C:/Users/JesseKlotz/.claude/squeez/hooks/pretooluse.sh\"";
        assert_eq!(hook_command_problem(cmd, present), None);
    }

    #[test]
    fn missing_script_is_reported() {
        let why = hook_command_problem("bash \"/h/.claude/squeez/hooks/x.sh\"", absent)
            .expect("must be flagged");
        assert!(why.contains("script not found"), "{why}");
    }

    /// A false FAIL is worse than a missed one, so anything that is not a plain
    /// `bash <script>` is left alone: bare paths (codex/gemini run their hooks
    /// directly) and the `bash -c '…'` adopted-HUD statusLine chain.
    #[test]
    fn non_plain_commands_are_left_alone() {
        assert_eq!(hook_command_problem("/h/.claude/squeez/hooks/x.sh", absent), None);
        assert_eq!(
            hook_command_problem(
                "bash -c 'input=$(cat); echo \"$input\" | { hud; } 2>/dev/null'",
                absent
            ),
            None
        );
        assert_eq!(hook_command_problem("bash two words here", absent), None);
    }

    #[test]
    fn registered_commands_are_collected_from_both_shapes_and_deduped() {
        let settings = crate::json_util::parse_value(
            r#"{
                 "hooks": {
                   "PreToolUse": [{"hooks":[{"command":"bash \"/h/squeez/hooks/pretooluse.sh\""}]}],
                   "Stop": [{"hooks":[{"command":"bash \"/h/squeez/buddy/shims/stop.sh\""}]}]
                 },
                 "PostToolUse": [{"hooks":[{"command":"bash \"/h/squeez/hooks/posttooluse.sh\""}]}],
                 "SessionStart": [{"hooks":[{"command":"bash /opt/other/hook.sh"}]}],
                 "statusLine": {"command":"bash \"/h/squeez/buddy/shims/statusline.sh\""}
               }"#,
        )
        .unwrap();
        let mut cmds = registered_squeez_commands(&settings);
        cmds.sort();
        assert_eq!(cmds.len(), 4, "foreign hook excluded, squeez ones kept: {cmds:?}");
        assert!(cmds.iter().all(|c| c.contains("squeez")));
        assert!(cmds.iter().any(|c| c.contains("statusline.sh")));
    }
}
