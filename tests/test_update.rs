use squeez::commands::update::{
    current_version, detect_target, find_expected_sha, install_atomic, verify_sha256,
};

#[test]
fn current_version_matches_cargo() {
    assert_eq!(current_version(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn detect_target_returns_known_value() {
    let t = detect_target();
    assert!(matches!(
        t,
        "macos-universal" | "linux-x86_64" | "linux-aarch64" | "windows-x86_64"
    ));
}

#[test]
fn find_expected_sha_two_column_format() {
    let text = "deadbeef  squeez-linux-x86_64\ncafebabe  squeez-macos-universal\n";
    assert_eq!(
        find_expected_sha(text, "squeez-linux-x86_64"),
        Some("deadbeef".to_string())
    );
    assert_eq!(
        find_expected_sha(text, "squeez-macos-universal"),
        Some("cafebabe".to_string())
    );
}

#[test]
fn find_expected_sha_missing_returns_none() {
    let text = "abc  squeez-linux-x86_64\n";
    assert!(find_expected_sha(text, "squeez-windows-x86_64.exe").is_none());
}

#[test]
fn find_expected_sha_skips_comments_and_blank() {
    let text = "# checksums\n\nabc123  squeez-x\n";
    assert_eq!(
        find_expected_sha(text, "squeez-x"),
        Some("abc123".to_string())
    );
}

#[test]
fn install_atomic_writes_target_with_content() {
    let dir = std::env::temp_dir().join(format!(
        "squeez_update_install_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("squeez");
    install_atomic(b"binary-content", &target).unwrap();
    let read = std::fs::read(&target).unwrap();
    assert_eq!(read, b"binary-content");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&target).unwrap().permissions().mode();
        assert!(mode & 0o111 != 0, "binary should be executable: {:o}", mode);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verify_sha256_known_vector_when_hasher_present() {
    // sha256("abc")
    let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let ok = verify_sha256(b"abc", expected);
    // Skip silently if neither sha256sum nor shasum -a 256 is available
    if which("sha256sum") || which("shasum") {
        assert!(ok, "sha256(\"abc\") known vector should match");
    }
}

#[test]
fn verify_sha256_mismatch_is_false() {
    if which("sha256sum") || which("shasum") {
        assert!(!verify_sha256(
            b"abc",
            "0000000000000000000000000000000000000000000000000000000000000000"
        ));
    }
}

fn which(name: &str) -> bool {
    std::process::Command::new(name)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
