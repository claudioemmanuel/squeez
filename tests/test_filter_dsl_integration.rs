//! Integration tests for E8's filter-DSL: layering (project overrides user)
//! and the `squeez filter-test` / `squeez wrap` CLI paths, spawning the
//! real binary (same pattern as test_flag_force.rs / test_success_collapse.rs).

use std::process::Command;

fn bin() -> String {
    env!("CARGO_BIN_EXE_squeez").to_string()
}

fn tmp_dir(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "squeez_filter_dsl_it_{}_{}",
        label,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn tmp_squeez_dir(label: &str) -> std::path::PathBuf {
    let dir = tmp_dir(&format!("{}_squeez", label));
    std::fs::create_dir_all(dir.join("sessions")).unwrap();
    std::fs::create_dir_all(dir.join("memory")).unwrap();
    dir
}

#[test]
fn filter_dsl_layering_project_overrides_user() {
    let squeez_dir = tmp_squeez_dir("layering");
    std::fs::write(squeez_dir.join("config.ini"), "net_win_min_tokens = 0\n").unwrap();
    // User-level filter: keeps only 1 line.
    std::fs::write(
        squeez_dir.join("filters.ini"),
        "[filter \"echomulti\"]\nhead = 1\n",
    )
    .unwrap();

    let project_dir = tmp_dir("layering_project");
    std::fs::create_dir_all(project_dir.join(".squeez")).unwrap();
    // Project-level filter for the SAME name: keeps 2 lines instead -- must win.
    std::fs::write(
        project_dir.join(".squeez").join("filters.ini"),
        "[filter \"echomulti\"]\nhead = 2\n",
    )
    .unwrap();

    let out = Command::new(bin())
        .args(["wrap", "echomulti() { printf 'one\\ntwo\\nthree\\n'; }; echomulti"])
        .current_dir(&project_dir)
        .env("SQUEEZ_DIR", &squeez_dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("one"), "got: {}", stdout);
    assert!(stdout.contains("two"), "project head=2 should win, got: {}", stdout);
    assert!(!stdout.contains("three"), "third line should be dropped by head=2, got: {}", stdout);

    std::fs::remove_dir_all(&squeez_dir).ok();
    std::fs::remove_dir_all(&project_dir).ok();
}

#[test]
fn filter_dsl_wired_into_wrap_for_matching_command() {
    let squeez_dir = tmp_squeez_dir("wired");
    std::fs::write(squeez_dir.join("config.ini"), "net_win_min_tokens = 0\n").unwrap();

    let project_dir = tmp_dir("wired_project");
    std::fs::create_dir_all(project_dir.join(".squeez")).unwrap();
    std::fs::write(
        project_dir.join(".squeez").join("filters.ini"),
        "[filter \"myfakecmd\"]\nstrip_lines_containing = \"DEBUG\"\n",
    )
    .unwrap();

    let out = Command::new(bin())
        .args([
            "wrap",
            "myfakecmd() { printf 'DEBUG: noise\\nreal output\\n'; }; myfakecmd",
        ])
        .current_dir(&project_dir)
        .env("SQUEEZ_DIR", &squeez_dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("real output"), "got: {}", stdout);
    assert!(!stdout.contains("DEBUG"), "got: {}", stdout);

    std::fs::remove_dir_all(&squeez_dir).ok();
    std::fs::remove_dir_all(&project_dir).ok();
}

#[test]
fn filter_test_cmd_reports_pass_and_fail() {
    let squeez_dir = tmp_squeez_dir("filtertest");
    let project_dir = tmp_dir("filtertest_project");
    std::fs::create_dir_all(project_dir.join(".squeez")).unwrap();
    std::fs::write(
        project_dir.join(".squeez").join("filters.ini"),
        "[filter \"greet\"]\n\
         replace = \"hello\" -> \"hi\"\n\
         test_input = \"hello world\"\n\
         test_expect = \"hi world\"\n\
         test_input = \"hello there\"\n\
         test_expect = \"totally wrong\"\n",
    )
    .unwrap();

    let out = Command::new(bin())
        .args(["filter-test"])
        .current_dir(&project_dir)
        .env("SQUEEZ_DIR", &squeez_dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("FAIL"), "got: {}", stdout);
    assert!(stdout.contains("1 passed, 1 failed"), "got: {}", stdout);
    assert_ne!(out.status.code(), Some(0), "exit code should be non-zero on failure");

    std::fs::remove_dir_all(&squeez_dir).ok();
    std::fs::remove_dir_all(&project_dir).ok();
}

#[test]
fn filter_test_cmd_all_pass_exits_zero() {
    let squeez_dir = tmp_squeez_dir("filtertest_ok");
    let project_dir = tmp_dir("filtertest_ok_project");
    std::fs::create_dir_all(project_dir.join(".squeez")).unwrap();
    std::fs::write(
        project_dir.join(".squeez").join("filters.ini"),
        "[filter \"greet\"]\n\
         replace = \"hello\" -> \"hi\"\n\
         test_input = \"hello world\"\n\
         test_expect = \"hi world\"\n",
    )
    .unwrap();

    let out = Command::new(bin())
        .args(["filter-test"])
        .current_dir(&project_dir)
        .env("SQUEEZ_DIR", &squeez_dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("1 passed, 0 failed"), "got: {}", stdout);
    assert_eq!(out.status.code(), Some(0));

    std::fs::remove_dir_all(&squeez_dir).ok();
    std::fs::remove_dir_all(&project_dir).ok();
}
