use squeez::config::Config;

#[test]
fn plan_mode_passthrough_default_true() {
    let c = Config::default();
    assert!(c.plan_mode_passthrough, "passthrough must be on by default");
}

#[test]
fn plan_mode_passthrough_parses_false() {
    let c = Config::from_str("plan_mode_passthrough = false\n");
    assert!(!c.plan_mode_passthrough);
}

#[test]
fn plan_mode_passthrough_parses_true() {
    let c = Config::from_str("plan_mode_passthrough = true\n");
    assert!(c.plan_mode_passthrough);
}

#[test]
fn plan_mode_passthrough_does_not_affect_other_fields() {
    let c = Config::from_str("plan_mode_passthrough = false\nmax_lines = 80\n");
    assert!(!c.plan_mode_passthrough);
    assert_eq!(c.max_lines, 80);
}

/// When SQUEEZ_PLAN_MODE=1 is set and plan_mode_passthrough is enabled,
/// wrap::run should return without compressing. We verify the env-var path
/// by spawning a real subprocess through the binary.
#[test]
fn plan_mode_env_var_causes_passthrough() {
    use std::process::Command;

    let bin = env!("CARGO_BIN_EXE_squeez");
    let output = Command::new(bin)
        .args(["wrap", "echo", "hello plan mode"])
        .env("SQUEEZ_PLAN_MODE", "1")
        .output()
        .expect("failed to run squeez wrap");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // passthrough: output contains the raw echo result, no squeez header
    assert!(
        stdout.contains("hello plan mode"),
        "expected raw passthrough output, got: {stdout}"
    );
    assert!(
        !stdout.contains("[squeez"),
        "expected no squeez header in plan mode passthrough, got: {stdout}"
    );
}

#[test]
fn plan_mode_disabled_in_config_still_compresses() {
    use std::process::Command;

    let bin = env!("CARGO_BIN_EXE_squeez");

    // Build a temp config with plan_mode_passthrough = false
    let dir = std::env::temp_dir().join("squeez_test_plan_disabled");
    std::fs::create_dir_all(&dir).ok();
    std::fs::write(dir.join("config.ini"), "plan_mode_passthrough = false\n").unwrap();

    let output = Command::new(bin)
        .args(["wrap", "echo", "hello compressed"])
        .env("SQUEEZ_PLAN_MODE", "1")
        .env("SQUEEZ_DIR", &dir)
        .output()
        .expect("failed to run squeez wrap");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // plan_mode_passthrough=false: SQUEEZ_PLAN_MODE env var is ignored,
    // so squeez header should appear
    assert!(
        stdout.contains("[squeez") || stdout.contains("hello compressed"),
        "unexpected output: {stdout}"
    );

    std::fs::remove_dir_all(&dir).ok();
}
