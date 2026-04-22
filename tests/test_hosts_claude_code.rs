use std::path::PathBuf;
use std::sync::Mutex;

use squeez::hosts::{ClaudeCodeAdapter, HostAdapter, HostCaps};

// HOME is process-global; serialise tests that mutate it.
static ENV_GUARD: Mutex<()> = Mutex::new(());

fn tmp_home() -> PathBuf {
    let uniq = format!(
        "{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        std::process::id()
    );
    let path = std::env::temp_dir().join(format!("squeez-claude-code-test-{uniq}"));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn with_home<F: FnOnce(&PathBuf) -> R, R>(f: F) -> R {
    let guard = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let home = tmp_home();
    let prev_home = std::env::var("HOME").ok();
    let prev_userprofile = std::env::var("USERPROFILE").ok();
    std::env::set_var("HOME", &home);
    std::env::remove_var("USERPROFILE");
    // ClaudeCodeAdapter uses SQUEEZ_DIR to override data_dir; clear it.
    std::env::remove_var("SQUEEZ_DIR");
    let r = f(&home);
    if let Some(h) = prev_home {
        std::env::set_var("HOME", h);
    } else {
        std::env::remove_var("HOME");
    }
    if let Some(u) = prev_userprofile {
        std::env::set_var("USERPROFILE", u);
    }
    drop(guard);
    r
}

fn python3_available() -> bool {
    std::process::Command::new("python3")
        .arg("--version")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

const EXPECTED_MATCHER: &str = "Bash|Read|Grep|Glob|Agent|Task";

#[test]
fn claude_code_capabilities_include_budget_hard() {
    let a = ClaudeCodeAdapter;
    let caps = a.capabilities();
    assert!(caps.contains(HostCaps::BASH_WRAP));
    assert!(caps.contains(HostCaps::SESSION_MEM));
    assert!(caps.contains(HostCaps::BUDGET_HARD));
}

#[test]
fn claude_code_install_pretooluse_matcher_covers_read_grep_glob_agent_task() {
    if !python3_available() {
        eprintln!("python3 unavailable — skipping install test");
        return;
    }
    with_home(|home| {
        // ClaudeCodeAdapter::is_installed() checks for ~/.claude; create it.
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        let a = ClaudeCodeAdapter;
        a.install(&PathBuf::from("/usr/local/bin/squeez"))
            .expect("install should succeed");

        let settings_path = home.join(".claude/settings.json");
        assert!(settings_path.exists(), "settings.json not created");
        let body = std::fs::read_to_string(&settings_path).unwrap();

        // The matcher value must appear verbatim in the JSON.
        assert!(
            body.contains(EXPECTED_MATCHER),
            "settings.json does not contain expected matcher '{EXPECTED_MATCHER}':\n{body}"
        );
        // The pretooluse hook command must be present.
        assert!(
            body.contains("pretooluse.sh"),
            "settings.json missing pretooluse.sh reference:\n{body}"
        );
        // Old bare "Bash" matcher must NOT appear as a standalone matcher value.
        // The JSON encodes it as `"matcher": "Bash"` — detect that exact string.
        assert!(
            !body.contains("\"matcher\": \"Bash\"") && !body.contains("\"matcher\":\"Bash\""),
            "old narrow 'Bash' matcher still present in settings.json:\n{body}"
        );
    });
}

#[test]
fn claude_code_install_is_idempotent_no_duplicate_pretooluse() {
    if !python3_available() {
        eprintln!("python3 unavailable — skipping idempotency test");
        return;
    }
    with_home(|home| {
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        let a = ClaudeCodeAdapter;
        a.install(&PathBuf::from("/usr/local/bin/squeez")).unwrap();
        a.install(&PathBuf::from("/usr/local/bin/squeez")).unwrap();

        let body = std::fs::read_to_string(home.join(".claude/settings.json")).unwrap();
        // "pretooluse.sh" should appear exactly once in the file.
        let count = body.matches("pretooluse.sh").count();
        assert_eq!(
            count, 1,
            "duplicate pretooluse.sh entries after second install:\n{body}"
        );
    });
}

#[test]
fn claude_code_install_upgrades_old_bash_only_matcher_in_place() {
    if !python3_available() {
        eprintln!("python3 unavailable — skipping upgrade-in-place test");
        return;
    }
    with_home(|home| {
        let claude_dir = home.join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();

        // Seed settings.json with the old-style matcher: "Bash".
        let hooks_dir = claude_dir.join("squeez/hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        let old_cmd = format!("bash {}/pretooluse.sh", hooks_dir.display());
        let old_json = format!(
            r#"{{"PreToolUse":[{{"matcher":"Bash","hooks":[{{"type":"command","command":"{old_cmd}"}}]}}]}}"#
        );
        std::fs::write(claude_dir.join("settings.json"), &old_json).unwrap();

        let a = ClaudeCodeAdapter;
        a.install(&PathBuf::from("/usr/local/bin/squeez"))
            .expect("install over old entry should succeed");

        let body = std::fs::read_to_string(claude_dir.join("settings.json")).unwrap();

        // Matcher must be upgraded.
        assert!(
            body.contains(EXPECTED_MATCHER),
            "matcher not upgraded to '{EXPECTED_MATCHER}':\n{body}"
        );
        // Only one pretooluse.sh reference — not duplicated.
        let count = body.matches("pretooluse.sh").count();
        assert_eq!(
            count, 1,
            "expected exactly one pretooluse.sh reference, got {count}:\n{body}"
        );
        // Old narrow matcher must be gone.
        assert!(
            !body.contains("\"matcher\": \"Bash\"") && !body.contains("\"matcher\":\"Bash\""),
            "old 'Bash' matcher still present after upgrade:\n{body}"
        );
    });
}
