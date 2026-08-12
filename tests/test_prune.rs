//! Integration coverage for `squeez prune` (#201) — the CLI hint
//! ("run squeez prune") referenced a command that fell through to the usage
//! banner because it was never dispatched. These tests run the real binary
//! against an isolated `SQUEEZ_DIR` to prove the subcommand now exists, wipes
//! expired blobs, and leaves fresh ones alone.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn tmp() -> PathBuf {
    static CTR: AtomicU64 = AtomicU64::new(0);
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "squeez_prune_test_{}_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        n
    ));
    std::fs::create_dir_all(d.join("blobs")).unwrap();
    d
}

fn run_prune(squeez_dir: &std::path::Path) -> String {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_squeez"))
        .arg("prune")
        .env("SQUEEZ_DIR", squeez_dir)
        .env("HOME", squeez_dir) // keep memory_dir/prune_old off the real $HOME too
        .output()
        .expect("run squeez prune");
    assert!(out.status.success(), "squeez prune exited nonzero: {out:?}");
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn prune_subcommand_is_dispatched_and_removes_only_expired_blobs() {
    let dir = tmp();
    let blobs = dir.join("blobs");

    // Fresh blob (id doesn't matter for this test, only mtime + shape).
    let fresh_id = "1111111111111111";
    std::fs::write(blobs.join(fresh_id), "fresh content").unwrap();

    // Expired blob, back-dated well past the default 7-day TTL.
    let stale_id = "2222222222222222";
    std::fs::write(blobs.join(stale_id), "stale content").unwrap();
    let past = std::time::SystemTime::now() - std::time::Duration::from_secs(8 * 86_400);
    let f = std::fs::File::open(blobs.join(stale_id)).unwrap();
    f.set_modified(past).unwrap();

    let stdout = run_prune(&dir);
    assert!(stdout.contains("squeez prune"), "unexpected output: {stdout}");
    assert!(stdout.contains("removed 1 expired blob"), "unexpected output: {stdout}");

    assert!(blobs.join(fresh_id).exists(), "fresh blob must survive prune");
    assert!(!blobs.join(stale_id).exists(), "expired blob must be removed");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn prune_subcommand_is_a_noop_on_an_empty_store() {
    let dir = tmp();
    let stdout = run_prune(&dir);
    assert!(stdout.contains("removed 0 expired blob"), "unexpected output: {stdout}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_subcommand_banner_no_longer_treats_prune_as_unknown() {
    // `squeez prune` with no matching setup must still exit 0 (it's a real,
    // dispatched subcommand now) rather than falling into the usage banner,
    // which exits 1.
    let dir = tmp();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_squeez"))
        .arg("prune")
        .env("SQUEEZ_DIR", &dir)
        .env("HOME", &dir)
        .output()
        .expect("run squeez prune");
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_dir_all(&dir);
}
