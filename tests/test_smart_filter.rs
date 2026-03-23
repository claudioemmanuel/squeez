use squeez::strategies::smart_filter::apply;

#[test]
fn removes_ansi_codes() {
    let input = vec!["\x1b[32mmodified\x1b[0m: src/main.rs".to_string()];
    assert_eq!(apply(input), vec!["modified: src/main.rs"]);
}

#[test]
fn removes_blank_lines() {
    let input = vec![
        "line one".to_string(),
        "".to_string(),
        "line two".to_string(),
    ];
    assert_eq!(apply(input), vec!["line one", "line two"]);
}

#[test]
fn removes_spinner_frames() {
    let input = vec!["⠋ loading...".to_string(), "actual output".to_string()];
    assert_eq!(apply(input), vec!["actual output"]);
}

#[test]
fn removes_progress_bars() {
    let input = vec!["████░░░░ 47%".to_string(), "done".to_string()];
    assert_eq!(apply(input), vec!["done"]);
}

#[test]
fn removes_git_hints() {
    let input = vec![
        "hint: use git push".to_string(),
        "On branch main".to_string(),
    ];
    assert_eq!(apply(input), vec!["On branch main"]);
}

#[test]
fn removes_npm_deprecation() {
    let input = vec![
        "npm warn deprecated lodash@4.17.21: use lodash-es".to_string(),
        "added 142 packages".to_string(),
    ];
    assert_eq!(apply(input), vec!["added 142 packages"]);
}

#[test]
fn strips_log_timestamps() {
    let input = vec!["[2026-03-22T14:23:01Z] INFO server started".to_string()];
    assert_eq!(apply(input), vec!["INFO server started"]);
}

#[test]
fn removes_node_modules_stack_frames() {
    let input = vec![
        "Error: ENOENT".to_string(),
        "    at Object.<anonymous> (/app/src/index.js:10:5)".to_string(),
        "    at Module._compile (/app/node_modules/some-pkg/lib/index.js:50:10)".to_string(),
        "    at Object.Module._extensions (/app/node_modules/other/index.js:1:1)".to_string(),
    ];
    let result = apply(input);
    assert!(result.iter().any(|l| l.contains("src/index.js")));
    assert!(!result.iter().any(|l| l.contains("node_modules")));
}

#[test]
fn passthrough_normal_lines() {
    let input = vec!["modified: src/auth.ts".to_string()];
    assert_eq!(apply(input), vec!["modified: src/auth.ts"]);
}
