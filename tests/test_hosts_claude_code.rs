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

#[test]
fn claude_code_install_registers_subagent_stop_precompact_postcompact() {
    if !python3_available() {
        eprintln!("python3 unavailable — skipping");
        return;
    }
    with_home(|home| {
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        let a = ClaudeCodeAdapter;
        a.install(&PathBuf::from("/usr/local/bin/squeez"))
            .expect("install should succeed");

        let body = std::fs::read_to_string(home.join(".claude/settings.json")).unwrap();
        assert!(body.contains("SubagentStop"), "SubagentStop missing from settings.json:\n{body}");
        assert!(body.contains("PreCompact"),   "PreCompact missing from settings.json:\n{body}");
        assert!(body.contains("PostCompact"),  "PostCompact missing from settings.json:\n{body}");
        assert!(body.contains("subagent-stop.sh"), "subagent-stop.sh cmd missing:\n{body}");
        assert!(body.contains("precompact.sh"),    "precompact.sh cmd missing:\n{body}");
        assert!(body.contains("postcompact.sh"),   "postcompact.sh cmd missing:\n{body}");
    });
}

#[test]
fn claude_code_uninstall_removes_subagent_stop_precompact_postcompact() {
    if !python3_available() {
        eprintln!("python3 unavailable — skipping");
        return;
    }
    with_home(|home| {
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        let a = ClaudeCodeAdapter;
        a.install(&PathBuf::from("/usr/local/bin/squeez")).unwrap();
        a.uninstall().expect("uninstall should succeed");

        let settings_path = home.join(".claude/settings.json");
        if settings_path.exists() {
            let body = std::fs::read_to_string(&settings_path).unwrap();
            assert!(!body.contains("subagent-stop.sh"), "subagent-stop.sh left after uninstall:\n{body}");
            assert!(!body.contains("precompact.sh"),    "precompact.sh left after uninstall:\n{body}");
            assert!(!body.contains("postcompact.sh"),   "postcompact.sh left after uninstall:\n{body}");
        }
    });
}

// Returns (top_level_squeez_count, nested_squeez_count) by parsing settings.json
// via python3 — keeps this crate zero-dep (no serde_json). Python indentation
// is significant, so the script is concat!'d with explicit "\n" — Rust's
// `\<newline>` continuation would strip the leading whitespace and break it.
fn count_squeez(settings_path: &std::path::Path) -> (usize, usize) {
    let script = concat!(
        "import json, sys\n",
        "EV=('PreToolUse','SessionStart','PostToolUse','SubagentStop','PreCompact','PostCompact')\n",
        "s=json.load(open(sys.argv[1]))\n",
        "def n(arr):\n",
        "    return sum(1 for m in (arr or []) if isinstance(m, dict) and any('squeez' in str(h.get('command','')) for h in (m.get('hooks') or [])))\n",
        "top=sum(n(s.get(e)) for e in EV if isinstance(s.get(e), list))\n",
        "nst=sum(n((s.get('hooks') or {}).get(e)) for e in EV if isinstance((s.get('hooks') or {}).get(e), list))\n",
        "print(f'{top}\\t{nst}')\n",
    );
    let out = std::process::Command::new("python3")
        .arg("-c")
        .arg(script)
        .arg(settings_path)
        .output()
        .expect("python3 probe failed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let mut parts = stdout.trim().split('\t');
    let top: usize = parts
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("probe stdout='{stdout}' stderr='{stderr}'"));
    let nested: usize = parts
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("probe stdout='{stdout}' stderr='{stderr}'"));
    (top, nested)
}

#[test]
fn claude_code_install_writes_hooks_into_hooks_object_not_top_level() {
    // Regression: earlier installs wrote hook entries at settings["PreToolUse"]
    // etc. Claude Code only reads from settings["hooks"][event], so the hooks
    // were silently inert. Lock down the correct shape.
    if !python3_available() {
        eprintln!("python3 unavailable — skipping");
        return;
    }
    with_home(|home| {
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        let a = ClaudeCodeAdapter;
        a.install(&PathBuf::from("/usr/local/bin/squeez")).unwrap();

        let (top, nested) = count_squeez(&home.join(".claude/settings.json"));
        assert_eq!(top, 0, "squeez entries must not live at top-level (Claude Code ignores them there)");
        assert_eq!(nested, 6, "expected 6 squeez entries under settings.hooks.* — got {nested}");
    });
}

#[test]
fn claude_code_install_migrates_legacy_top_level_hooks() {
    // Older squeez wrote hooks at the top level. A re-install must migrate
    // them into settings.hooks.* without duplication.
    if !python3_available() {
        eprintln!("python3 unavailable — skipping");
        return;
    }
    with_home(|home| {
        let claude_dir = home.join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        let hooks_dir = claude_dir.join("squeez/hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        let legacy_cmd = format!("bash {}/pretooluse.sh", hooks_dir.display());
        let seed = format!(
            r#"{{"PreToolUse":[{{"matcher":"Bash|Read|Grep|Glob|Agent|Task","hooks":[{{"type":"command","command":"{legacy_cmd}"}}]}}]}}"#
        );
        let settings_path = claude_dir.join("settings.json");
        std::fs::write(&settings_path, &seed).unwrap();

        let a = ClaudeCodeAdapter;
        a.install(&PathBuf::from("/usr/local/bin/squeez")).unwrap();

        let (top, nested) = count_squeez(&settings_path);
        assert_eq!(top, 0, "legacy top-level entries should be gone after migration");
        assert_eq!(nested, 6, "expected 6 nested squeez entries after migration");
        let body = std::fs::read_to_string(&settings_path).unwrap();
        assert_eq!(
            body.matches("pretooluse.sh").count(), 1,
            "pretooluse.sh duplicated after migration:\n{body}"
        );
    });
}

#[test]
fn claude_code_uninstall_removes_legacy_top_level_hooks() {
    // Users upgrading from broken installs may have squeez entries at the
    // top level. uninstall() must clean both shapes.
    if !python3_available() {
        eprintln!("python3 unavailable — skipping");
        return;
    }
    with_home(|home| {
        let claude_dir = home.join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        let hooks_dir = claude_dir.join("squeez/hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        let cmd = format!("bash {}/pretooluse.sh", hooks_dir.display());
        let seed = format!(
            r#"{{"PreToolUse":[{{"matcher":"Bash","hooks":[{{"type":"command","command":"{cmd}"}}]}}]}}"#
        );
        std::fs::write(claude_dir.join("settings.json"), &seed).unwrap();

        let a = ClaudeCodeAdapter;
        a.uninstall().expect("uninstall should succeed");

        if let Ok(body) = std::fs::read_to_string(claude_dir.join("settings.json")) {
            assert!(
                !body.contains("pretooluse.sh"),
                "legacy top-level squeez entry left after uninstall:\n{body}"
            );
        }
    });
}

#[test]
fn claude_code_install_is_idempotent_for_new_hooks() {
    if !python3_available() {
        eprintln!("python3 unavailable — skipping");
        return;
    }
    with_home(|home| {
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        let a = ClaudeCodeAdapter;
        a.install(&PathBuf::from("/usr/local/bin/squeez")).unwrap();
        a.install(&PathBuf::from("/usr/local/bin/squeez")).unwrap();

        let body = std::fs::read_to_string(home.join(".claude/settings.json")).unwrap();
        // Each new hook should appear exactly once
        for script in &["subagent-stop.sh", "precompact.sh", "postcompact.sh"] {
            let count = body.matches(script).count();
            assert_eq!(count, 1, "{script} appears {count}× after double install (want 1):\n{body}");
        }
    });
}
