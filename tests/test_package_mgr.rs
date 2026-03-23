use squeez::commands::{package_mgr::PackageMgrHandler, Handler};
use squeez::config::Config;

#[test]
fn removes_deprecation_warnings() {
    let lines = vec![
        "npm warn deprecated lodash@4.17.21: use lodash-es".to_string(),
        "npm warn deprecated rimraf@2.7.1: please update".to_string(),
        "added 142 packages in 4.2s".to_string(),
    ];
    let result = PackageMgrHandler.compress("npm install", lines, &Config::default());
    assert!(!result.iter().any(|l| l.contains("deprecated")));
    assert!(result.iter().any(|l| l.contains("added 142")));
}

#[test]
fn npm_audit_truncated() {
    let lines: Vec<String> = (0..200).map(|i| format!("vulnerability {}", i)).collect();
    let result = PackageMgrHandler.compress("npm audit", lines, &Config::default());
    assert!(result.len() <= 52);
}
