use std::path::Path;

use crate::session::{self, CurrentSession};

/// Entry point called from main.rs: `squeez track <tool> <bytes>`
pub fn run(tool: &str, bytes: &str) -> i32 {
    run_with_dir(tool, bytes, &session::sessions_dir())
}

/// Testable version that accepts an explicit sessions directory.
pub fn run_with_dir(tool: &str, bytes: &str, sessions_dir: &Path) -> i32 {
    let tokens = bytes.parse::<u64>().unwrap_or(0) / 4;
    let mut current = match CurrentSession::load(sessions_dir) {
        Some(s) => s,
        None => return 0, // no session initialised — silent no-op
    };
    current.total_tokens += tokens;
    current.save(sessions_dir);

    let event = format!(
        "{{\"type\":\"tool\",\"tool\":\"{}\",\"tokens_est\":{},\"ts\":{}}}",
        tool.replace('"', ""),
        tokens,
        session::unix_now(),
    );
    session::append_event(sessions_dir, &current.session_file, &event);
    0
}
