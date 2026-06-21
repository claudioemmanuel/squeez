// Verifies that new tool/command names added to filter::detect() route without
// panic and produce reasonable output. We test via filter::compress() since
// detect() is private.
use squeez::config::Config;
use squeez::filter;

fn cfg() -> Config {
    Config::default()
}

fn lines(s: &str) -> Vec<String> {
    s.lines().map(String::from).collect()
}

#[test]
fn bfs_routes_without_panic() {
    let out = filter::compress(
        "bfs /tmp -name '*.rs'",
        lines("/tmp/src/main.rs\n/tmp/src/lib.rs"),
        &cfg(),
    );
    assert!(!out.is_empty(), "bfs should return output, not panic");
}

#[test]
fn ugrep_routes_without_panic() {
    let out = filter::compress(
        "ugrep -r 'fn main' src/",
        lines("src/main.rs:1:fn main() {}\nsrc/lib.rs:5:fn main() {}"),
        &cfg(),
    );
    assert!(!out.is_empty(), "ugrep should return output, not panic");
}

#[test]
fn monitor_routes_without_panic() {
    let out = filter::compress(
        "monitor",
        lines("event: heartbeat\nevent: progress\nevent: done"),
        &cfg(),
    );
    assert!(!out.is_empty(), "monitor should return output, not panic");
}

#[test]
fn bfs_output_is_handled_like_find() {
    // bfs uses FsHandler. The file names are the payload, so a listing under
    // the truncation limit passes through verbatim (see #148).
    let many_files: Vec<String> = (0..10)
        .map(|i| format!("/project/src/file{i}.rs"))
        .collect();
    let out = filter::compress("bfs /project/src -name '*.rs'", many_files.clone(), &cfg());
    assert_eq!(out, many_files, "listing under the limit should pass through unchanged");
}

#[test]
fn ls_listing_is_not_collapsed_to_modified_count() {
    // Regression for #148: `ls`/`find` listings must NOT be collapsed to a
    // per-dir "N modified" count. The names are the answer the caller asked
    // for, and nothing was "modified" — that verb is borrowed from git-status.
    let files: Vec<String> = (0..8)
        .map(|i| format!("src/datamodel/Booking/file{i}.rs"))
        .collect();
    let out = filter::compress("ls -1 src/datamodel/Booking/", files.clone(), &cfg());
    assert!(
        !out.iter().any(|l| l.contains("modified")),
        "listing must not be mislabeled as a change-set: {out:?}"
    );
    assert!(
        !out.iter().any(|l| l.contains("[squeez grouped]")),
        "listing must not be collapsed to a count: {out:?}"
    );
    // Every name is still visible (8 < find_max_results default of 50).
    assert_eq!(out, files, "names are the payload and must pass through");
}

#[test]
fn long_find_listing_is_truncated_not_grouped() {
    // A genuinely long listing is bounded by truncation (visible head + a
    // truncation marker), never collapsed to a single dir count.
    let files: Vec<String> = (0..120).map(|i| format!("deep/dir/f{i}.rs")).collect();
    let out = filter::compress("find deep -type f", files, &cfg());
    assert!(out.len() < 120, "long listing should be bounded");
    assert!(
        out.iter().any(|l| l.contains("truncated")),
        "expected a truncation marker, got: {out:?}"
    );
    assert!(
        !out.iter().any(|l| l.contains("modified") || l.contains("[squeez grouped]")),
        "must truncate, not group: {out:?}"
    );
    // The first names are still visible verbatim.
    assert_eq!(out[0], "deep/dir/f0.rs");
}

#[test]
fn ugrep_output_handled_like_grep() {
    let grep_output = lines(
        "src/main.rs:10:fn run() {}\nsrc/main.rs:20:fn stop() {}\nsrc/lib.rs:5:fn init() {}",
    );
    let out = filter::compress("ugrep -n 'fn ' src/", grep_output, &cfg());
    assert!(!out.is_empty(), "ugrep output should be returned by TextProcHandler");
}

#[test]
fn unknown_command_still_returns_output() {
    let out = filter::compress(
        "some-new-unknown-tool --flag",
        lines("line one\nline two"),
        &cfg(),
    );
    assert!(!out.is_empty(), "unknown commands should fall through to GenericHandler");
}
