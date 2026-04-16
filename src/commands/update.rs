// `squeez update` — self-updater. Zero-dep: shells out to `curl` and
// `sha256sum` / `shasum -a 256` (both already required by install.sh).

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::json_util;
use crate::session::home_dir;

const REPO: &str = "claudioemmanuel/squeez";

pub fn run(args: &[String]) -> i32 {
    let mut check_only = false;
    let mut insecure = false;
    for a in args {
        match a.as_str() {
            "--check" => check_only = true,
            "--insecure" => insecure = true,
            "-h" | "--help" => {
                print_help();
                return 0;
            }
            other => {
                eprintln!("squeez update: unknown flag {}", other);
                return 2;
            }
        }
    }

    let current = current_version();

    let latest = match fetch_latest_tag() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("squeez update: failed to fetch latest release: {}", e);
            return 1;
        }
    };

    let latest_clean = latest.trim_start_matches('v');
    if latest_clean == current {
        println!("squeez {}: already up to date", current);
        return 0;
    }

    if check_only {
        println!("squeez update: {} → {}", current, latest_clean);
        return 0;
    }

    // Cargo-managed install: delegate to `cargo install` which handles
    // binary replacement atomically (no Windows exe-lock issues).
    if is_cargo_managed() {
        return update_via_cargo(latest_clean);
    }

    let target = detect_target();
    let asset_name = asset_name_for(target);
    let base = base_url();
    let asset_url = format!("{}/releases/download/{}/{}", base, latest, asset_name);
    let sha_url = format!("{}/releases/download/{}/checksums.sha256", base, latest);

    println!("squeez update: downloading {}...", asset_name);
    let bytes = match curl(&asset_url) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("squeez update: download failed: {}", e);
            return 1;
        }
    };

    if !insecure {
        let sha_text = match curl(&sha_url) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) => {
                eprintln!("squeez update: failed to fetch checksums: {}", e);
                return 1;
            }
        };
        let expected = match find_expected_sha(&sha_text, &asset_name) {
            Some(s) => s,
            None => {
                eprintln!("squeez update: no checksum entry for {}", asset_name);
                return 1;
            }
        };
        if !verify_sha256(&bytes, &expected) {
            eprintln!("squeez update: SHA256 mismatch — refusing to install");
            return 1;
        }
        println!("squeez update: SHA256 ok");
    } else {
        eprintln!("squeez update: --insecure: skipping checksum verification");
    }

    let target_path = install_target_path();
    let immediate = match install_atomic(&bytes, &target_path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("squeez update: install failed: {}", e);
            return 1;
        }
    };

    if immediate {
        println!("squeez update: installed {} → {}", current, latest_clean);
        // Re-register hooks in settings.json (path may have changed, or first-time setup)
        if let Err(e) = crate::commands::setup::register_claude_settings() {
            eprintln!("squeez update: warning: could not update settings.json: {}", e);
        }
    } else {
        println!("squeez update: {} → {} queued — restart to apply", current, latest_clean);
    }

    0
}

fn print_help() {
    println!("squeez update — self-update from GitHub releases");
    println!();
    println!("Usage:");
    println!("  squeez update            Download and install latest");
    println!("  squeez update --check    Report whether an update is available");
    println!("  squeez update --insecure Skip SHA256 verification (NOT recommended)");
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn base_url() -> String {
    if let Ok(o) = std::env::var("SQUEEZ_UPDATE_URL_OVERRIDE") {
        return o;
    }
    format!("https://github.com/{}", REPO)
}

fn api_base() -> String {
    if let Ok(o) = std::env::var("SQUEEZ_UPDATE_API_OVERRIDE") {
        return o;
    }
    format!("https://api.github.com/repos/{}", REPO)
}

pub fn detect_target() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos-universal"
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        "linux-x86_64"
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "aarch64") {
        "linux-aarch64"
    } else if cfg!(target_os = "windows") {
        "windows-x86_64"
    } else {
        "unknown"
    }
}

fn asset_name_for(target: &str) -> String {
    if target == "windows-x86_64" {
        format!("squeez-{}.exe", target)
    } else {
        format!("squeez-{}", target)
    }
}

fn is_cargo_managed() -> bool {
    std::env::current_exe()
        .ok()
        .map(|p| {
            let s = p.to_string_lossy();
            s.contains(".cargo/bin") || s.contains(".cargo\\bin")
        })
        .unwrap_or(false)
}

fn update_via_cargo(version: &str) -> i32 {
    println!("squeez update: cargo install detected — running cargo install squeez@{}...", version);
    let status = std::process::Command::new("cargo")
        .args(["install", "squeez", "--version", version])
        .status();
    match status {
        Ok(s) if s.success() => {
            println!("squeez update: installed {} via cargo", version);
            if let Err(e) = crate::commands::setup::register_claude_settings() {
                eprintln!("squeez update: warning: could not update settings.json: {}", e);
            }
            0
        }
        Ok(s) => {
            eprintln!("squeez update: cargo install failed (exit {})", s.code().unwrap_or(-1));
            1
        }
        Err(e) => {
            eprintln!("squeez update: could not run cargo: {}", e);
            1
        }
    }
}

fn install_target_path() -> PathBuf {
    // Always update the binary we're actually running from.
    if let Ok(exe) = std::env::current_exe() {
        return exe;
    }
    // Fallback: canonical hooks location
    let dir = format!("{}/.claude/squeez/bin", home_dir());
    PathBuf::from(dir).join(if cfg!(windows) { "squeez.exe" } else { "squeez" })
}

// ── Network ────────────────────────────────────────────────────────────────

fn fetch_latest_tag() -> Result<String, String> {
    // Try /releases/latest API endpoint first.
    let url = format!("{}/releases/latest", api_base());
    let body = curl(&url)?;
    let s = String::from_utf8_lossy(&body);
    if let Some(tag) = json_util::extract_str(&s, "tag_name") {
        return Ok(tag);
    }
    // Fallback for file:// overrides used in tests
    if let Some(tag) = s.lines().find(|l| l.starts_with("v")).map(String::from) {
        return Ok(tag.trim().to_string());
    }
    Err("no tag_name in release JSON".to_string())
}

pub fn curl(url: &str) -> Result<Vec<u8>, String> {
    let out = Command::new("curl")
        .args(["-fsSL", "-A", "squeez-update", url])
        .output()
        .map_err(|e| format!("curl spawn: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "curl exit {}: {}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}

// ── SHA256 verification ────────────────────────────────────────────────────

pub fn find_expected_sha(text: &str, filename: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Format: "<hex>  <filename>"
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?;
        if name.ends_with(filename) || name == filename {
            return Some(hash.to_string());
        }
    }
    None
}

pub fn verify_sha256(bytes: &[u8], expected_hex: &str) -> bool {
    if let Some(actual) = compute_sha256(bytes) {
        actual.eq_ignore_ascii_case(expected_hex)
    } else {
        false
    }
}

fn compute_sha256(bytes: &[u8]) -> Option<String> {
    // Try sha256sum then shasum -a 256
    for (cmd, args) in [
        ("sha256sum", vec![]),
        ("shasum", vec!["-a", "256"]),
    ] {
        if let Some(hash) = run_hasher(cmd, &args, bytes) {
            return Some(hash);
        }
    }
    None
}

fn run_hasher(cmd: &str, args: &[&str], input: &[u8]) -> Option<String> {
    use std::io::Write;
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(input).ok()?;
    }
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    s.split_whitespace().next().map(|h| h.to_string())
}

// ── Install ────────────────────────────────────────────────────────────────

/// Returns `Ok(true)` when the binary was replaced immediately,
/// `Ok(false)` when the replacement was deferred (Windows self-update).
pub fn install_atomic(bytes: &[u8], target: &Path) -> Result<bool, String> {
    let parent = target.parent().ok_or("target has no parent")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let staging = parent.join(format!(
        "{}.new",
        target.file_name().and_then(|s| s.to_str()).unwrap_or("squeez")
    ));
    std::fs::write(&staging, bytes).map_err(|e| format!("write staging: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
        std::fs::rename(&staging, target).map_err(|e| format!("rename: {}", e))?;
        return Ok(true);
    }

    #[cfg(windows)]
    {
        let is_self = std::env::current_exe()
            .ok()
            .and_then(|p| p.canonicalize().ok())
            .zip(target.canonicalize().ok())
            .map(|(a, b)| a == b)
            .unwrap_or(false);

        if !is_self {
            // Target is not the running binary — direct rename is safe.
            std::fs::rename(&staging, target).map_err(|e| format!("rename: {}", e))?;
            return Ok(true);
        }

        // Self-update: try the rename dance (Windows allows renaming a running exe).
        let bak = target.with_extension("exe.bak");
        let _ = std::fs::remove_file(&bak);
        if std::fs::rename(target, &bak).is_ok() {
            match std::fs::rename(&staging, target) {
                Ok(()) => {
                    let _ = std::fs::remove_file(&bak);
                    return Ok(true);
                }
                Err(e) => {
                    let _ = std::fs::rename(&bak, target);
                    return Err(format!("rename new->target failed: {}", e));
                }
            }
        }

        // Rename dance failed (target locked) — spawn a detached cmd.exe that
        // moves the staged file into place after this process exits.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let cmd_str = format!(
            "ping -n 2 127.0.0.1 > nul && move /Y \"{}\" \"{}\"",
            staging.display(),
            target.display()
        );
        let spawned = std::process::Command::new("cmd")
            .args(["/c", &cmd_str])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .is_ok();
        if spawned {
            eprintln!("squeez update: update scheduled — binary replaces itself on exit");
        } else {
            eprintln!("squeez update: wrote {} — run to complete:", staging.display());
            eprintln!("  move /Y \"{}\" \"{}\"", staging.display(), target.display());
        }
        return Ok(false);
    }

    #[allow(unreachable_code)]
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_target_returns_known() {
        let t = detect_target();
        assert!(matches!(
            t,
            "macos-universal" | "linux-x86_64" | "linux-aarch64" | "windows-x86_64"
        ));
    }

    #[test]
    fn asset_name_windows_has_exe() {
        assert!(asset_name_for("windows-x86_64").ends_with(".exe"));
        assert!(!asset_name_for("linux-x86_64").ends_with(".exe"));
    }

    #[test]
    fn find_expected_sha_parses_standard_format() {
        let text = "abc123  squeez-linux-x86_64\nf00d  squeez-macos-universal\n";
        assert_eq!(
            find_expected_sha(text, "squeez-linux-x86_64"),
            Some("abc123".to_string())
        );
        assert_eq!(
            find_expected_sha(text, "squeez-macos-universal"),
            Some("f00d".to_string())
        );
        assert_eq!(find_expected_sha(text, "squeez-windows-x86_64.exe"), None);
    }

    #[test]
    fn find_expected_sha_skips_blank_and_comments() {
        let text = "# header\n\nabcd  squeez-x\n";
        assert_eq!(find_expected_sha(text, "squeez-x"), Some("abcd".to_string()));
    }

    #[test]
    fn verify_sha256_known_vector() {
        // sha256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        let ok = verify_sha256(
            b"abc",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        );
        // Skip on systems without shasum/sha256sum
        if compute_sha256(b"abc").is_some() {
            assert!(ok);
        }
    }

    #[test]
    fn verify_sha256_mismatch_returns_false() {
        if compute_sha256(b"x").is_some() {
            assert!(!verify_sha256(b"x", "0000000000000000000000000000000000000000000000000000000000000000"));
        }
    }

    #[test]
    fn install_atomic_writes_target() {
        let dir = std::env::temp_dir().join(format!(
            "squeez_update_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("squeez");
        install_atomic(b"#!/bin/sh\necho test\n", &target).unwrap();
        let content = std::fs::read(&target).unwrap();
        assert_eq!(content, b"#!/bin/sh\necho test\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
