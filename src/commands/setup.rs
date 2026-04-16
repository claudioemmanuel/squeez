// `squeez setup` — post-install setup for cargo/npm users.
//
// Creates ~/.claude/squeez/ directory structure, copies the running binary
// to the canonical hooks location, downloads hook scripts from GitHub, and
// registers hooks + statusline in ~/.claude/settings.json.

use std::path::PathBuf;

use crate::session::home_dir;

const REPO_RAW: &str = "https://raw.githubusercontent.com/claudioemmanuel/squeez/main";

const HOOKS: &[&str] = &[
    "pretooluse.sh",
    "session-start.sh",
    "posttooluse.sh",
    "copilot-pretooluse.sh",
    "copilot-session-start.sh",
    "copilot-posttooluse.sh",
];

// Default config written to ~/.claude/squeez/config.ini on first install.
// Never overwritten on subsequent runs — preserves user customizations.
const DEFAULT_CONFIG_INI: &str = "\
# squeez configuration — edit to customize\n\
# https://github.com/claudioemmanuel/squeez\n\
\n\
enabled = true\n\
show_header = true\n\
\n\
# Persona: off | lite | full | ultra\n\
# full  = caveman mode (~75% token cut, drop articles, fragments OK)\n\
# ultra = max compression + abbreviation substitutions (default)\n\
persona = ultra\n\
\n\
# Compression limits\n\
max_lines = 120\n\
git_log_max_commits = 20\n\
git_diff_max_lines = 150\n\
docker_logs_max_lines = 100\n\
find_max_results = 50\n\
\n\
# Context engine\n\
adaptive_intensity = true\n\
context_cache_enabled = true\n\
redundancy_cache_enabled = true\n\
summarize_threshold_lines = 300\n\
dedup_min = 2\n\
read_max_lines = 300\n\
grep_max_results = 100\n\
\n\
# Memory\n\
memory_retention_days = 30\n\
auto_compress_md = true\n\
lang = en\n\
";

// Python script that registers squeez hooks + statusline in ~/.claude/settings.json.
// Mirrors the registration block in install.sh.
const REGISTER_SETTINGS_PY: &str = r#"
import json, os, sys

path = os.path.expanduser("~/.claude/settings.json")
settings = {}
try:
    if os.path.exists(path):
        with open(path) as f:
            settings = json.load(f)
except (json.JSONDecodeError, IOError) as e:
    print("Warning: could not read settings.json: " + str(e), file=sys.stderr)

def ensure_list(key):
    if not isinstance(settings.get(key), list):
        settings[key] = []

ensure_list("PreToolUse")
pre = {"matcher": "Bash", "hooks": [{"type": "command", "command": "bash ~/.claude/squeez/hooks/pretooluse.sh"}]}
if not any("squeez" in str(h) for h in settings["PreToolUse"]):
    settings["PreToolUse"].append(pre)

ensure_list("SessionStart")
start = {"hooks": [{"type": "command", "command": "bash ~/.claude/squeez/hooks/session-start.sh"}]}
if not any("squeez" in str(h) for h in settings["SessionStart"]):
    settings["SessionStart"].append(start)

ensure_list("PostToolUse")
post = {"hooks": [{"type": "command", "command": "bash ~/.claude/squeez/hooks/posttooluse.sh"}]}
if not any("squeez" in str(h) for h in settings["PostToolUse"]):
    settings["PostToolUse"].append(post)

existing_status = settings.get("statusLine", {})
existing_cmd = existing_status.get("command", "") if isinstance(existing_status, dict) else ""
squeez_cmd = "bash ~/.claude/squeez/bin/statusline.sh"
if "squeez" not in existing_cmd:
    if existing_cmd:
        new_cmd = "bash -c 'input=$(cat); echo \"$input\" | { " + existing_cmd.rstrip() + "; } 2>/dev/null; echo \"$input\" | " + squeez_cmd + "'"
        settings["statusLine"] = {"type": "command", "command": new_cmd}
    else:
        settings["statusLine"] = {"type": "command", "command": squeez_cmd}

os.makedirs(os.path.dirname(path), exist_ok=True)
tmp = path + ".tmp"
with open(tmp, "w") as f:
    json.dump(settings, f, indent=2)
os.replace(tmp, path)
print("settings.json updated.")
"#;

/// Detect the user's preferred language by inspecting ~/.claude/CLAUDE.md
/// and system locale env vars. Returns a squeez lang code (e.g. "pt-BR", "en").
fn detect_lang(home: &str) -> &'static str {
    // 1. ~/.claude/CLAUDE.md — most reliable signal for Claude Code users
    let claude_md = format!("{}/.claude/CLAUDE.md", home);
    if let Ok(content) = std::fs::read_to_string(&claude_md) {
        let lower = content.to_lowercase();
        if lower.contains("pt-br") || lower.contains("pt_br")
            || lower.contains("português") || lower.contains("portugues")
        {
            return "pt-BR";
        }
    }

    // 2. System locale env vars
    for var in &["LANG", "LC_ALL", "LANGUAGE"] {
        if let Ok(val) = std::env::var(var) {
            let lower = val.to_lowercase();
            if lower.starts_with("pt_br") || lower.starts_with("pt-br") {
                return "pt-BR";
            }
            if lower.starts_with("pt") {
                return "pt-BR"; // treat any pt locale as pt-BR (only variant with assets)
            }
        }
    }

    "en"
}

pub fn run(args: &[String]) -> i32 {
    let force = args.iter().any(|a| a == "--force" || a == "-f");

    let home = home_dir();
    let install_dir = format!("{}/.claude/squeez", home);

    // 1. Create directory structure
    for sub in &["bin", "hooks", "sessions", "memory"] {
        let path = format!("{}/{}", install_dir, sub);
        if let Err(e) = std::fs::create_dir_all(&path) {
            eprintln!("squeez setup: failed to create {}: {}", path, e);
            return 1;
        }
    }

    // 2. Write default config.ini if not present (never overwrite user customizations)
    let config_path = format!("{}/config.ini", install_dir);
    if !std::path::Path::new(&config_path).exists() {
        let lang = detect_lang(&home);
        let content = DEFAULT_CONFIG_INI.replace("lang = en", &format!("lang = {}", lang));
        if lang != "en" {
            println!("squeez setup: detected language → {}", lang);
        }
        if let Err(e) = std::fs::write(&config_path, content) {
            eprintln!("squeez setup: warning: could not write config.ini: {}", e);
        } else {
            println!("squeez setup: config.ini created → {}", config_path);
        }
    } else {
        println!("squeez setup: existing config.ini preserved → {}", config_path);
    }

    // 3. Copy self binary to canonical hooks location
    let bin_name = if cfg!(windows) { "squeez.exe" } else { "squeez" };
    let target_bin = PathBuf::from(format!("{}/bin/{}", install_dir, bin_name));

    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("squeez setup: cannot determine current exe path: {}", e);
            return 1;
        }
    };

    let already_in_place = current_exe
        .canonicalize()
        .ok()
        .zip(target_bin.canonicalize().ok())
        .map(|(a, b)| a == b)
        .unwrap_or(false);

    if !already_in_place || force {
        if let Err(e) = std::fs::copy(&current_exe, &target_bin) {
            eprintln!("squeez setup: failed to copy binary to {}: {}", target_bin.display(), e);
            return 1;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&target_bin, std::fs::Permissions::from_mode(0o755));
        }
        println!("squeez setup: binary installed → {}", target_bin.display());
    } else {
        println!("squeez setup: binary already at {}", target_bin.display());
    }

    // 4. Download hook scripts from GitHub
    println!("squeez setup: downloading hooks...");
    for hook in HOOKS {
        let url = format!("{}/hooks/{}", REPO_RAW, hook);
        let dest = format!("{}/hooks/{}", install_dir, hook);
        match crate::commands::update::curl(&url) {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(&dest, &bytes) {
                    eprintln!("squeez setup: failed to write hook {}: {}", hook, e);
                    return 1;
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
                }
            }
            Err(e) => {
                eprintln!("squeez setup: failed to download hook {}: {}", hook, e);
                return 1;
            }
        }
    }

    // 5. Download statusline.sh
    let statusline_url = format!("{}/scripts/statusline.sh", REPO_RAW);
    let statusline_dest = format!("{}/bin/statusline.sh", install_dir);
    match crate::commands::update::curl(&statusline_url) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&statusline_dest, &bytes) {
                eprintln!("squeez setup: warning: could not write statusline.sh: {}", e);
            } else {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&statusline_dest, std::fs::Permissions::from_mode(0o755));
                }
            }
        }
        Err(e) => eprintln!("squeez setup: warning: could not download statusline.sh: {}", e),
    }

    // 6. Register hooks in ~/.claude/settings.json
    println!("squeez setup: registering hooks in settings.json...");
    if let Err(e) = register_claude_settings() {
        eprintln!("squeez setup: failed to update settings.json: {}", e);
        return 1;
    }

    let version = crate::commands::update::current_version();
    println!("squeez setup: done — squeez {} ready. Restart Claude Code to activate.", version);
    0
}

/// Registers squeez hooks and statusline in ~/.claude/settings.json.
/// Called by both `squeez setup` and `squeez update`.
pub fn register_claude_settings() -> Result<(), String> {
    run_python(REGISTER_SETTINGS_PY)
}

fn run_python(script: &str) -> Result<(), String> {
    use std::io::Write;

    // Try python3 first (Unix/modern Windows), then python (older Windows)
    for python in &["python3", "python"] {
        let mut child = match std::process::Command::new(python)
            .arg("-")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn()
        {
            Ok(c) => c,
            Err(_) => continue,
        };

        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(script.as_bytes());
        }

        let status = child.wait().map_err(|e| e.to_string())?;
        if status.success() {
            return Ok(());
        }
        return Err("python script exited with error".to_string());
    }

    Err("python3/python not found — cannot update settings.json".to_string())
}

fn print_help() {
    println!("squeez setup — configure hooks after cargo/npm install");
    println!();
    println!("Usage:");
    println!("  squeez setup           Install hooks and register in settings.json");
    println!("  squeez setup --force   Force re-copy binary even if already in place");
    println!();
    println!("Use this after: cargo install squeez  OR  npm i -g squeez");
    println!("Equivalent to running: curl -fsSL <install.sh url> | sh -s -- --setup-only");
}

pub fn run_with_help(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return 0;
    }
    run(args)
}
