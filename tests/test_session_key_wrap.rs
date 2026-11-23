//! A Bash call from a Claude Code session is wrapped with that session's id,
//! so the `wrap` process writes the same per-session context file as the
//! hooks. One test per file: the session key is process-global.
use squeez::commands::hook::{pre_tool_use, HookEnv};
use squeez::config::Config;

#[test]
fn wrapped_command_carries_the_session() {
    squeez::context::cache::set_session_key("11111111-2222-3333-4444-555555555555");
    let dir = std::env::temp_dir().join(format!("squeez-keywrap-{}", std::process::id()));
    let env = HookEnv {
        squeez_bin: "/bin/squeez".into(),
        ledger: dir.join("ledger"),
        sessions_dir: dir.clone(),
        max_inflight: None,
        now: 0,
    };
    let raw = r#"{"session_id":"x","tool_name":"Bash","tool_input":{"command":"git status"}}"#;
    let out = pre_tool_use(raw, &env, &Config::default()).expect("wrapped");
    assert!(
        out.contains("/bin/squeez wrap --session 11111111-2222-3333-4444-555555555555 'git status'"),
        "{out}"
    );
}
