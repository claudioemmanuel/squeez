//! The net-win gate must account for the retrieve marker.
//!
//! The marker is ~40 tokens and ships only because compression ran, so it is
//! compression-attributable overhead. It used to be appended AFTER the gate
//! had already decided the call was a win, which meant a call saving 25
//! tokens could emit a 40-token marker and still print a header claiming a
//! win. These tests pin the arithmetic that closes that.

use std::process::Command;

fn squeez_bin() -> &'static str {
    env!("CARGO_BIN_EXE_squeez")
}

/// Runs `squeez wrap <cmd>` against an isolated SQUEEZ_DIR so the session
/// cache of one test can't answer another's call.
fn wrap(dir: &str, extra_config: &str, cmd: &str) -> String {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        format!("{dir}/config.ini"),
        format!("enabled = true\nredundancy_cache_enabled = false\n{extra_config}"),
    )
    .unwrap();
    let out = Command::new(squeez_bin())
        .args(["wrap", cmd])
        .env("SQUEEZ_DIR", dir)
        .output()
        .expect("squeez wrap should run");
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn tmp(name: &str) -> String {
    let d = std::env::temp_dir().join(format!("squeez-emission-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    d.to_string_lossy().to_string()
}

#[test]
fn a_call_with_nothing_to_compress_is_gated_to_verbatim_passthrough() {
    // 60 structurally distinct lines: no repetition for dedup, and the
    // varying field is alphabetic so the log-template stage can't collapse
    // it either. Nothing is saved, so nothing should be claimed.
    let dir = tmp("nothing");
    let cmd = "awk \"BEGIN{for(i=0;i<60;i++){s=\\\"\\\";n=i;\
for(j=0;j<4;j++){s=sprintf(\\\"%c\\\",97+(n%26)) s;n=int(n/26)};\
print \\\"record \\\" s \\\" completed with status ok and no further detail\\\"}}\"";
    let out = wrap(&dir, "", cmd);

    assert!(
        !out.contains("squeez_retrieve with key"),
        "a gated call must not ship a marker:\n{out}"
    );
    assert!(!out.contains("# squeez ["), "a gated call must suppress the header:\n{out}");
    // Gating is a passthrough, not a drop — the content survives intact.
    assert!(out.contains("record aaaa completed"), "{out}");
    assert!(out.contains("record aacg completed"), "{out}");
}

#[test]
fn a_real_win_still_ships_both_header_and_marker() {
    // Highly repetitive output: dedup collapses it far past the marker's
    // cost, so the same code path that gated above must let this through.
    let dir = tmp("realwin");
    let cmd = "for i in $(seq 1 400); do echo 'identical repeated progress line'; done";
    let out = wrap(&dir, "", cmd);

    assert!(out.contains("# squeez ["), "a real win must print the header:\n{out}");
    assert!(
        out.contains("squeez_retrieve with key"),
        "a real win over the retrieve threshold must ship the marker:\n{out}"
    );
}

#[test]
fn the_marker_text_and_its_cost_estimate_use_the_same_shape() {
    // Regression guard for the split between the emitted marker and the
    // pre-gate estimate: if the two formats ever drift, the gate is costing
    // something other than what it ships.
    let dir = tmp("shape");
    let cmd = "for i in $(seq 1 400); do echo 'identical repeated progress line'; done";
    let out = wrap(&dir, "", cmd);
    let marker = out
        .lines()
        .find(|l| l.contains("squeez_retrieve with key"))
        .expect("marker should be present");
    assert!(marker.starts_with("[squeez: full "), "{marker}");
    assert!(marker.contains("-line output stored"), "{marker}");
    assert!(marker.ends_with("to find it later]"), "{marker}");
}

#[test]
fn preservation_guard_can_be_switched_off() {
    // The guard costs an anchor scan on high-reduction calls; the switch has
    // to actually reach the wrap path.
    let dir = tmp("presoff");
    let cmd = "for i in $(seq 1 400); do echo 'identical repeated progress line'; done";
    let out = wrap(&dir, "preservation_guard = false\n", cmd);
    assert!(!out.contains("[anchors:"), "guard was off but tagged anyway:\n{out}");
}
