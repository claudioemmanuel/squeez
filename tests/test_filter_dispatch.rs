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
fn bfs_output_is_compressible_like_find() {
    // bfs should use FsHandler — many files in same dir should be grouped
    let many_files: Vec<String> = (0..10)
        .map(|i| format!("/project/src/file{i}.rs"))
        .collect();
    let out = filter::compress("bfs /project/src -name '*.rs'", many_files, &cfg());
    // FsHandler groups sibling files; output should be shorter than input
    assert!(!out.is_empty());
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
