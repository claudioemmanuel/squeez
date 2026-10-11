//! Process-level proof for the instruction-file write guard
//! (`squeez::hosts::memory_file`): real `squeez init --host=opencode`
//! processes racing on one `AGENTS.md`, as two sessions starting in the same
//! second do. Every process runs in a sandbox — home, config, state and
//! working directory all point into a temp dir.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use squeez::hosts::memory_file;

const BIN: &str = env!("CARGO_BIN_EXE_squeez");
const PROCS: usize = 8;
const SENTINEL: &str = "## END SENTINEL — owner rules end here\n";

fn sandbox(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "squeez-write-race-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for sub in ["home", "config/opencode", "cwd"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    dir
}

fn rules(lines: usize) -> String {
    format!(
        "# Owner rules\n\n## Shell policy\nuse bash\n\n## MCP servers\nuse them\n{}{SENTINEL}",
        "- a rule the owner wrote and squeez must never lose\n".repeat(lines)
    )
}

/// One writer's state directory, with a config that renders a block no other
/// writer renders — sessions with different summaries write different blocks.
fn state_dir(root: &Path, n: usize) -> PathBuf {
    let state = root.join(format!("state-{n}"));
    let ini = format!(
        "auto_compress_md = false\ncompact_threshold_tokens = {}\n",
        100_000 + n * 8_000
    );
    // Where the guarded build keeps OpenCode state …
    let new_home = state.join("squeez").join("opencode");
    std::fs::create_dir_all(&new_home).unwrap();
    std::fs::write(new_home.join("config.ini"), &ini).unwrap();
    // … and where a stock build looks for it.
    let old_home = root.join("config/opencode/squeez");
    std::fs::create_dir_all(&old_home).unwrap();
    std::fs::write(old_home.join("config.ini"), "auto_compress_md = false\n").unwrap();
    state
}

fn spawn_init(bin: &str, root: &Path, state: &Path) -> Child {
    let home = root.join("home");
    Command::new(bin)
        .args(["init", "--host=opencode"])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("LOCALAPPDATA", state)
        .env("XDG_DATA_HOME", state)
        .env("SQUEEZ_DIR", home.join(".claude").join("squeez"))
        .current_dir(root.join("cwd"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn squeez init")
}

/// Start `PROCS` inits at once, `rounds` times. Returns the first round after
/// which the rules were no longer in the file.
fn race(bin: &str, root: &Path, rules: &str, rounds: usize) -> Option<usize> {
    let agents = root.join("config/opencode/AGENTS.md");
    let states: Vec<PathBuf> = (0..PROCS).map(|n| state_dir(root, n)).collect();
    for round in 0..rounds {
        let children: Vec<Child> = states.iter().map(|s| spawn_init(bin, root, s)).collect();
        for mut child in children {
            child.wait().expect("squeez init did not exit");
        }
        // Every writer has exited: this is the settled file.
        let settled = std::fs::read_to_string(&agents).unwrap_or_default();
        if !settled.ends_with(rules) {
            return Some(round);
        }
    }
    None
}

#[test]
fn concurrent_init_processes_keep_the_owner_rules() {
    let root = sandbox("fixed");
    let agents = root.join("config/opencode/AGENTS.md");
    let rules = rules(6000);
    std::fs::write(&agents, &rules).unwrap();

    assert_eq!(race(BIN, &root, &rules, 25), None, "owner rules lost");

    let end = std::fs::read_to_string(&agents).unwrap();
    assert!(end.starts_with("<!-- squeez:start -->"), "no squeez block was written");
    assert_eq!(end.matches("<!-- squeez:start -->").count(), 1);
    assert_eq!(end.matches("<!-- squeez:end -->").count(), 1);
    assert!(end.ends_with(&rules), "owner rules not byte-identical");
    let names: Vec<String> = std::fs::read_dir(agents.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(!names.iter().any(|n| n.contains("squeez-tmp-")), "temp file left: {names:?}");
    assert!(names.iter().any(|n| n == ".AGENTS.md.squeez-lock"), "no lock file: {names:?}");
}

/// The same race against a binary without the guard, named by
/// `SQUEEZ_LEGACY_BIN`. Opt-in: the stock binary is not part of this repo.
#[test]
fn legacy_binary_loses_the_owner_rules_in_the_same_race() {
    let Ok(legacy) = std::env::var("SQUEEZ_LEGACY_BIN") else {
        eprintln!("skipped: SQUEEZ_LEGACY_BIN is not set");
        return;
    };
    let rules = rules(6000);
    for attempt in 0..10 {
        let root = sandbox("legacy");
        let agents = root.join("config/opencode/AGENTS.md");
        std::fs::write(&agents, &rules).unwrap();
        if let Some(round) = race(&legacy, &root, &rules, 25) {
            let after = std::fs::read_to_string(&agents).unwrap_or_default();
            eprintln!(
                "legacy binary lost the owner rules: attempt {attempt}, round {round}, \
                 AGENTS.md settled at {} bytes (rules alone are {})",
                after.len(),
                rules.len()
            );
            assert!(!after.contains(SENTINEL));
            return;
        }
    }
    panic!("{legacy} did not reproduce the content loss");
}

/// Not a test of its own: the body of the child that
/// `lock_of_a_killed_process_does_not_block_the_next_writer` starts.
#[test]
fn lock_holder_child() {
    let Ok(target) = std::env::var("SQUEEZ_TEST_HOLD_LOCK") else {
        return;
    };
    let target = PathBuf::from(target);
    let _held = memory_file::open(&target).expect("child takes the lock");
    std::fs::write(target.with_extension("ready"), "held").unwrap();
    std::thread::sleep(Duration::from_secs(120));
}

#[test]
fn lock_of_a_killed_process_does_not_block_the_next_writer() {
    let root = sandbox("deadlock");
    let target = root.join("AGENTS.md");
    let rules = rules(10);
    std::fs::write(&target, &rules).unwrap();

    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "lock_holder_child", "--nocapture", "--test-threads=1"])
        .env("SQUEEZ_TEST_HOLD_LOCK", &target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let ready = target.with_extension("ready");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready.exists() {
        assert!(Instant::now() < deadline, "child never took the lock");
        std::thread::sleep(Duration::from_millis(20));
    }

    // Held by a live process: the next writer is kept out and writes nothing.
    let err = memory_file::open_with_wait(&target, Duration::from_millis(300))
        .err()
        .expect("lock must be held by the child");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), rules);

    // The holder dies without releasing anything.
    child.kill().unwrap();
    child.wait().unwrap();

    let file = memory_file::open_with_wait(&target, Duration::from_secs(5))
        .expect("a dead process's lock must not block the next writer");
    assert_eq!(file.existing(), rules);
}
