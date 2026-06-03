// Layer 2 — Skill tool output rewriting in PostToolUse. When Claude invokes a
// skill via the Skill tool, squeez compresses the (prose-heavy) skill body and
// dedups a second identical injection to a reference note.

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

// A prose-heavy skill body (10+ lines) modelled on the real DEBUG_error-detective
// skill: substitutable words (with/because/function/configuration/documentation)
// so Ultra compression clears the 64-byte savings gate.
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

#[test]
fn skill_body_is_compressed_on_first_injection() {
    let dir = tmp();
    let cfg = Config::default();
    let body = skill_body();
    let json = skill_json(&body);

    let rewrite = compute_rewrite(&json, "Skill", &dir, &cfg);
    assert!(rewrite.is_some(), "first skill injection should be compressed");
    let out = rewrite.unwrap();
    assert!(out.len() < body.len(), "compressed body must be smaller");
    assert!(out.contains("fn"), "function→fn substitution expected");
    assert!(out.contains("w/"), "with→w/ substitution expected");
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
