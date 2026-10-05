use std::path::{Path, PathBuf};
use std::sync::Mutex;

use squeez::hosts::{settings_json, CopilotCliAdapter, HostAdapter};

// HOME is process-global; tests that mutate it must serialise.
static ENV_GUARD: Mutex<()> = Mutex::new(());

const EVENTS: [&str; 3] = ["PreToolUse", "SessionStart", "PostToolUse"];

fn tmp_home() -> PathBuf {
    let uniq = format!(
        "{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        std::process::id()
    );
    let path = std::env::temp_dir().join(format!("squeez-copilot-test-{uniq}"));
    std::fs::create_dir_all(path.join(".copilot")).unwrap();
    path
}

fn with_home<F: FnOnce(&PathBuf) -> R, R>(f: F) -> R {
    let guard = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let home = tmp_home();
    let prev_home = std::env::var("HOME").ok();
    let prev_userprofile = std::env::var("USERPROFILE").ok();
    let prev_squeez_dir = std::env::var("SQUEEZ_DIR").ok();
    std::env::set_var("HOME", &home);
    std::env::remove_var("USERPROFILE");
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
    if let Some(d) = prev_squeez_dir {
        std::env::set_var("SQUEEZ_DIR", d);
    }
    std::fs::remove_dir_all(&home).ok();
    drop(guard);
    r
}

fn settings_path(home: &Path) -> PathBuf {
    home.join(".copilot/settings.json")
}

/// What squeez ≤ 1.48.10 wrote: the event map at the top level, where the
/// Copilot CLI ignores it, with an unquoted script path.
const LEGACY_SETTINGS: &str = r#"{
  "model": "gpt-5",
  "PreToolUse": [
    {"matcher": "Bash", "hooks": [{"type": "command", "command": "bash /h/.copilot/squeez/hooks/copilot-pretooluse.sh"}]},
    {"hooks": [{"type": "command", "command": "bash /h/mine/audit.sh"}]}
  ],
  "SessionStart": [{"hooks": [{"type": "command", "command": "bash /h/.copilot/squeez/hooks/copilot-session-start.sh"}]}],
  "PostToolUse": [{"hooks": [{"type": "command", "command": "bash /h/.copilot/squeez/hooks/copilot-posttooluse.sh"}]}]
}"#;

#[test]
fn copilot_install_registers_hooks_under_the_hooks_key() {
    with_home(|home| {
        CopilotCliAdapter.install(Path::new("/unused")).expect("install");

        let settings = settings_json::load_lenient(&settings_path(home)).expect("settings");
        let hooks = settings.get("hooks").expect("hooks object");
        for event in EVENTS {
            assert!(hooks.get(event).is_some(), "{event} missing under hooks");
            assert!(settings.get(event).is_none(), "{event} written at the top level");
        }
    });
}

#[test]
fn copilot_install_quotes_the_script_path() {
    with_home(|home| {
        CopilotCliAdapter.install(Path::new("/unused")).expect("install");

        let raw = std::fs::read_to_string(settings_path(home)).unwrap();
        let script = settings_json::shell_arg(
            &home.join(".copilot/squeez/hooks/copilot-pretooluse.sh"),
        );
        // The settings file holds the command JSON-escaped.
        let expected = format!("bash {}", script).replace('"', "\\\"");
        assert!(raw.contains(&expected), "expected `{expected}` in:\n{raw}");
    });
}

#[test]
fn copilot_install_moves_legacy_top_level_entries() {
    with_home(|home| {
        std::fs::write(settings_path(home), LEGACY_SETTINGS).unwrap();

        CopilotCliAdapter.install(Path::new("/unused")).expect("install");

        let settings = settings_json::load_lenient(&settings_path(home)).expect("settings");
        assert!(settings.get("SessionStart").is_none());
        assert!(settings.get("PostToolUse").is_none());
        assert!(settings.get("model").is_some(), "unrelated setting lost");
        let raw = std::fs::read_to_string(settings_path(home)).unwrap();
        assert!(raw.contains("/h/mine/audit.sh"), "foreign hook lost:\n{raw}");
        assert!(
            !raw.contains("bash /h/.copilot/squeez/hooks/"),
            "legacy squeez entry left behind:\n{raw}"
        );
        let hooks = settings.get("hooks").expect("hooks object");
        for event in EVENTS {
            assert!(hooks.get(event).is_some(), "{event} missing under hooks");
        }
    });
}

#[test]
fn copilot_uninstall_removes_nested_and_legacy_entries() {
    with_home(|home| {
        std::fs::write(settings_path(home), LEGACY_SETTINGS).unwrap();
        CopilotCliAdapter.install(Path::new("/unused")).expect("install");
        // Put a legacy entry back, as a pre-fix `squeez setup` would have.
        std::fs::write(
            settings_path(home),
            std::fs::read_to_string(settings_path(home)).unwrap().replacen(
                "{",
                r#"{"SessionStart": [{"hooks": [{"type": "command", "command": "bash /h/.copilot/squeez/hooks/copilot-session-start.sh"}]}],"#,
                1,
            ),
        )
        .unwrap();

        CopilotCliAdapter.uninstall().expect("uninstall");

        let raw = std::fs::read_to_string(settings_path(home)).unwrap();
        assert!(!raw.contains("squeez"), "squeez entry survived uninstall:\n{raw}");
        assert!(raw.contains("/h/mine/audit.sh"), "foreign hook lost:\n{raw}");
    });
}
