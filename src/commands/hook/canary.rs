//! Host-contract canary.
//!
//! squeez was dead for three days in July 2026 because Claude Code renamed the
//! PostToolUse field `tool_result` to `tool_response`: every hook still ran,
//! read nothing, and failed open in silence. Failing open is right — a hook
//! must never break a tool call — but failing open *silently* is not.
//!
//! Each Claude Code tool hook checks the payload for the fields it depends on.
//! A missing field is recorded once per session: a `[squeez: …]` warning is
//! queued for the model (drained by the next wrapped command), and a line is
//! appended to `host_mismatch.log`, which `squeez doctor` reports.

use std::io::Write;
use std::path::Path;

use crate::context::cache::SessionContext;
use crate::json_util::JsonValue;

/// File under the sessions dir that `squeez doctor` reads.
pub const LOG_FILE: &str = "host_mismatch.log";

/// Keep the log bounded: past this size it is truncated to its newer half.
const LOG_MAX_BYTES: u64 = 64 * 1024;

/// What is wrong with a payload, or `None` when it has the expected shape.
/// `event` is `pre-tool-use` or `post-tool-use`.
pub fn mismatch(event: &str, raw: &str, parsed: Option<&JsonValue>) -> Option<String> {
    if raw.trim().is_empty() {
        return None; // nothing was sent; not a contract change
    }
    let d = match parsed {
        Some(d) if d.is_obj() => d,
        _ => return Some("payload is not a JSON object".to_string()),
    };
    if d.get("tool_name").and_then(JsonValue::as_str).is_none() {
        return Some("missing `tool_name`".to_string());
    }
    match event {
        "pre-tool-use" if d.get("tool_input").is_none() => {
            Some("missing `tool_input`".to_string())
        }
        "post-tool-use" if d.get("tool_response").is_none() => Some(
            if d.get("tool_result").is_some() {
                "legacy `tool_result` instead of `tool_response`"
            } else {
                "missing `tool_response`"
            }
            .to_string(),
        ),
        _ => None,
    }
}

/// Record `problem` for `event` once per session: warning + log line.
pub fn record(sessions_dir: &Path, event: &str, problem: &str, now: i64) {
    let mut first = false;
    SessionContext::update(sessions_dir, |ctx| {
        // `nudged_keys` outlives a session, so the epoch scopes the key to one.
        let key = format!("host:{}:{event}:{problem}", ctx.session_epoch);
        if ctx.nudged_keys.iter().any(|k| k == &key) {
            return;
        }
        ctx.nudged_keys.push(key);
        ctx.queue_warning(&format!(
            "host payload changed ({event}: {problem}) — compression may be off; \
             run `squeez update` then `squeez doctor`"
        ));
        first = true;
    });
    if first {
        append_log(sessions_dir, &format!("{now}\t{event}\t{problem}"));
    }
}

fn append_log(sessions_dir: &Path, line: &str) {
    let path = sessions_dir.join(LOG_FILE);
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > LOG_MAX_BYTES {
        if let Ok(s) = std::fs::read_to_string(&path) {
            let keep: Vec<&str> = s.lines().collect();
            let half = keep.len() / 2;
            let _ = std::fs::write(&path, keep[half..].join("\n") + "\n");
        }
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{line}");
    }
}

/// Entries newer than `since` (unix seconds), newest last: `(ts, event, problem)`.
pub fn recent(sessions_dir: &Path, since: i64) -> Vec<(i64, String, String)> {
    let Ok(s) = std::fs::read_to_string(sessions_dir.join(LOG_FILE)) else {
        return Vec::new();
    };
    s.lines()
        .filter_map(|l| {
            let mut it = l.splitn(3, '\t');
            let ts = it.next()?.parse::<i64>().ok()?;
            Some((ts, it.next()?.to_string(), it.next()?.to_string()))
        })
        .filter(|(ts, _, _)| *ts >= since)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json_util::parse_value;

    fn check(event: &str, raw: &str) -> Option<String> {
        mismatch(event, raw, parse_value(raw).as_ref())
    }

    #[test]
    fn current_payloads_pass() {
        let post = r#"{"tool_name":"Read","tool_input":{},"tool_response":{"content":"x"}}"#;
        assert_eq!(check("post-tool-use", post), None);
        let pre = r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#;
        assert_eq!(check("pre-tool-use", pre), None);
    }

    #[test]
    fn legacy_tool_result_is_flagged() {
        // The exact shape that killed squeez for three days.
        let raw = r#"{"tool_name":"Bash","tool_input":{},"tool_result":"out"}"#;
        let p = check("post-tool-use", raw).unwrap();
        assert!(p.contains("tool_result"), "{p}");
    }

    #[test]
    fn missing_fields_and_garbage_are_flagged() {
        assert!(check("post-tool-use", r#"{"tool_name":"Bash"}"#).is_some());
        assert!(check("pre-tool-use", r#"{"tool_name":"Bash"}"#).is_some());
        assert!(check("pre-tool-use", r#"{"tool_input":{}}"#).is_some());
        assert!(check("post-tool-use", "not json").is_some());
        assert_eq!(check("post-tool-use", "   "), None);
    }

    #[test]
    fn records_once_per_session_and_doctor_can_read_it() {
        let dir = std::env::temp_dir().join(format!("squeez-canary-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        record(&dir, "post-tool-use", "missing `tool_response`", 100);
        record(&dir, "post-tool-use", "missing `tool_response`", 101);
        let ctx = SessionContext::load(&dir);
        assert_eq!(ctx.pending_warnings.len(), 1, "{:?}", ctx.pending_warnings);
        assert!(ctx.pending_warnings[0].contains("host payload changed"));
        let got = recent(&dir, 0);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, 100);
        assert!(recent(&dir, 200).is_empty());
        // A new session warns again.
        SessionContext::update(&dir, |c| c.begin_session("next"));
        record(&dir, "post-tool-use", "missing `tool_response`", 300);
        assert_eq!(recent(&dir, 0).len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
