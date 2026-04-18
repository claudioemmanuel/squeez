use squeez::hosts::{all_hosts, find, HostCaps};

#[test]
fn registry_contains_five_adapters() {
    let hosts = all_hosts();
    assert_eq!(hosts.len(), 5, "expected 5 host adapters");
}

#[test]
fn all_host_names_are_distinct() {
    let mut names: Vec<&'static str> = all_hosts().iter().map(|h| h.name()).collect();
    names.sort();
    let before = names.len();
    names.dedup();
    assert_eq!(before, names.len(), "duplicate host name in registry");
}

#[test]
fn all_hosts_expose_bash_wrap_and_session_mem() {
    for h in all_hosts() {
        let c = h.capabilities();
        assert!(c.contains(HostCaps::BASH_WRAP), "{} missing BASH_WRAP", h.name());
        assert!(c.contains(HostCaps::SESSION_MEM), "{} missing SESSION_MEM", h.name());
    }
}

#[test]
fn claude_copilot_opencode_expose_budget_hard() {
    for name in &["claude-code", "copilot", "opencode"] {
        let a = find(name).expect(name);
        assert!(
            a.capabilities().contains(HostCaps::BUDGET_HARD),
            "{} should expose BUDGET_HARD",
            name
        );
    }
}

#[test]
fn gemini_and_codex_expose_budget_soft() {
    for name in &["gemini", "codex"] {
        let a = find(name).expect(name);
        assert!(
            a.capabilities().contains(HostCaps::BUDGET_SOFT),
            "{} should expose BUDGET_SOFT",
            name
        );
    }
}

#[test]
fn find_known_slugs() {
    for slug in &["claude-code", "copilot", "opencode", "gemini", "codex"] {
        assert!(find(slug).is_some(), "find({}) returned None", slug);
    }
}

#[test]
fn find_unknown_slug_returns_none() {
    assert!(find("random-cli").is_none());
}

#[test]
fn data_dirs_are_distinct() {
    // Pin HOME to a deterministic value so the test is hermetic.
    std::env::remove_var("SQUEEZ_DIR");
    std::env::remove_var("XDG_CONFIG_HOME");
    std::env::set_var("HOME", "/tmp/squeez-host-registry-test");
    let mut dirs: Vec<String> = all_hosts()
        .iter()
        .map(|h| h.data_dir().to_string_lossy().into_owned())
        .collect();
    dirs.sort();
    let before = dirs.len();
    dirs.dedup();
    assert_eq!(before, dirs.len(), "data_dir collisions across hosts");
}
