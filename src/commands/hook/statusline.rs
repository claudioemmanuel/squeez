//! The Claude Code status line: `squeez | -12345tk 40% | 87 calls | 2 agents | Eff: 61.3%`.
//!
//! Claude Code re-runs the status line on every render, so this is the hook
//! that ran most often — and it started a Python interpreter up to five times
//! a run. Same files, same segments, same rounding (both round half to even).

use std::path::Path;

use crate::json_util::{self, JsonValue};

/// The line, or `None` when there is no session yet (the script printed
/// nothing at all then, not even `squeez`).
pub fn render(squeez_dir: &Path) -> Option<String> {
    let sessions = squeez_dir.join("sessions");
    let session_file = sessions.join("current.json");
    if !session_file.is_file() {
        return None;
    }
    let session = read_obj(&session_file);
    let saved = int_field(session.as_ref(), "tokens_saved");
    let calls = int_field(session.as_ref(), "total_calls");

    let context = read_obj(&sessions.join("context.json"));
    let spawns = int_field(context.as_ref(), "agent_spawns");
    let total_in: i64 = ["tokens_bash", "tokens_read", "tokens_grep", "tokens_other"]
        .iter()
        .map(|k| int_field(context.as_ref(), k))
        .sum();

    let efficiency_bp = last_efficiency_bp(&squeez_dir.join("memory/summaries.jsonl"));

    let mut parts = vec!["squeez".to_string()];
    if saved > 0 {
        if total_in > 0 {
            let ratio = saved as f64 * 100.0 / total_in as f64;
            parts.push(format!("| -{saved}tk {ratio:.0}%"));
        } else {
            parts.push(format!("| -{saved}tk"));
        }
    }
    if calls > 0 {
        parts.push(format!("| {calls} calls"));
    }
    if spawns > 0 {
        parts.push(format!("| {spawns} agents"));
    }
    if efficiency_bp > 0 {
        parts.push(format!("| Eff: {:.1}%", efficiency_bp as f64 / 100.0));
    }
    Some(parts.join(" "))
}

fn read_obj(path: &Path) -> Option<JsonValue> {
    let body = std::fs::read_to_string(path).ok()?;
    json_util::parse_value(&body).filter(JsonValue::is_obj)
}

/// An integer field, 0 when absent or not an integer. (A float reached
/// bash's `-gt` as `12.5` and failed the test, so it never showed either.)
fn int_field(d: Option<&JsonValue>, key: &str) -> i64 {
    match d.and_then(|d| d.get(key)) {
        Some(JsonValue::Num(n)) if n.fract() == 0.0 && n.abs() < 1e15 => *n as i64,
        _ => 0,
    }
}

/// `efficiency_overall_bp` of the last line of `summaries.jsonl`.
fn last_efficiency_bp(path: &Path) -> i64 {
    let Ok(body) = std::fs::read_to_string(path) else {
        return 0;
    };
    // Python's `readlines()[-1]`: a trailing blank line is the last line.
    let last = body.split_inclusive('\n').next_back().unwrap_or("").trim();
    let parsed = json_util::parse_value(last).filter(JsonValue::is_obj);
    int_field(parsed.as_ref(), "efficiency_overall_bp")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("squeez-statusline-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sessions")).unwrap();
        std::fs::create_dir_all(d.join("memory")).unwrap();
        d
    }

    #[test]
    fn no_session_prints_nothing() {
        let d = dir("none");
        assert_eq!(render(&d), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn renders_every_segment_with_python_rounding() {
        let d = dir("full");
        std::fs::write(
            d.join("sessions/current.json"),
            r#"{"tokens_saved":5,"total_calls":87}"#,
        )
        .unwrap();
        std::fs::write(
            d.join("sessions/context.json"),
            r#"{"agent_spawns":2,"tokens_bash":2,"tokens_read":2,"tokens_grep":0,"tokens_other":0}"#,
        )
        .unwrap();
        std::fs::write(
            d.join("memory/summaries.jsonl"),
            "{\"efficiency_overall_bp\":1}\n{\"efficiency_overall_bp\":6125}\n",
        )
        .unwrap();
        // 5*100/4 = 125.0; 6125/100 = 61.25 -> "61.2" (half to even on the binary value)
        assert_eq!(
            render(&d).unwrap(),
            "squeez | -5tk 125% | 87 calls | 2 agents | Eff: 61.2%"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn broken_files_degrade_to_the_bare_name() {
        let d = dir("broken");
        std::fs::write(d.join("sessions/current.json"), "nope").unwrap();
        std::fs::write(
            d.join("memory/summaries.jsonl"),
            "{\"efficiency_overall_bp\":10}\n\n",
        )
        .unwrap();
        assert_eq!(render(&d).unwrap(), "squeez");
        std::fs::write(d.join("sessions/current.json"), r#"{"tokens_saved":7}"#).unwrap();
        assert_eq!(render(&d).unwrap(), "squeez | -7tk");
        let _ = std::fs::remove_dir_all(&d);
    }
}
