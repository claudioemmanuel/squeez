// Layer 2 — Skill tool output handling in PostToolUse. When Claude invokes a
// skill via the Skill tool, squeez keeps the first injection full-size (skill
// bodies are structured checklists, not prose — lexical compression nets ~1%)
// and dedups any later identical injection to a reference note via a
// session-long store that is NOT bounded by the 16-call redundancy window.

use squeez::commands::compress_output::compute_rewrite;
use squeez::config::Config;
use std::sync::atomic::{AtomicU64, Ordering};

fn tmp() -> std::path::PathBuf {
    static CTR: AtomicU64 = AtomicU64::new(0);
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "squeez_skill_out_{}_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        n
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

// A skill body (10+ lines) modelled on the real DEBUG_error-detective skill.
fn skill_body() -> String {
    let mut s = String::from(
        "You are a senior error detective with expertise in analyzing error patterns.\n",
    );
    for i in 0..12 {
        s.push_str(&format!(
            "Step {i}: configure the function with these parameters because of the documentation and the configuration.\n"
        ));
    }
    s
}

fn skill_json(body: &str) -> String {
    let esc = body
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!(r#"{{"tool_name":"Skill","tool_result":{{"content":"{}"}}}}"#, esc)
}

fn read_json(n: usize) -> String {
    // Distinct multi-line Read output to advance call_log / call_counter.
    let body = format!("line one of read {n}\nline two of read {n}\nline three {n}");
    format!(
        r#"{{"tool_name":"Read","tool_result":{{"content":"{}"}}}}"#,
        body.replace('\n', "\\n")
    )
}

#[test]
fn first_skill_injection_is_recorded_not_rewritten() {
    let dir = tmp();
    let cfg = Config::default();
    let json = skill_json(&skill_body());

    // First injection: recorded in the session-long store, body kept full-size.
    let rewrite = compute_rewrite(&json, "Skill", &dir, &cfg);
    assert!(
        rewrite.is_none(),
        "first skill injection should be kept full-size, got: {rewrite:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn duplicate_skill_injection_is_deduped() {
    let dir = tmp();
    let cfg = Config::default();
    let json = skill_json(&skill_body());

    // First call records the original body.
    let _ = compute_rewrite(&json, "Skill", &dir, &cfg);
    // Second identical call must collapse to a reference note.
    let second = compute_rewrite(&json, "Skill", &dir, &cfg);
    assert!(second.is_some(), "duplicate injection should be rewritten");
    let note = second.unwrap();
    assert!(
        note.contains("identical to Skill"),
        "expected dedup note, got: {note}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn skill_dedup_survives_beyond_recent_window() {
    // The session-long skill store must catch a re-injection even when far more
    // than `recent_window` (16) other tool calls intervene — exactly the case
    // the windowed `redundancy::check` would miss.
    let dir = tmp();
    let cfg = Config::default();
    let json = skill_json(&skill_body());

    // First injection.
    assert!(compute_rewrite(&json, "Skill", &dir, &cfg).is_none());

    // 25 distinct Read calls push the skill far outside any 16-call window.
    for n in 0..25 {
        let _ = compute_rewrite(&read_json(n), "Read", &dir, &cfg);
    }

    // Re-injection still dedups.
    let again = compute_rewrite(&json, "Skill", &dir, &cfg);
    assert!(
        again.as_deref().is_some_and(|s| s.contains("identical to Skill")),
        "re-injection after 25 calls should still dedup, got: {again:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
