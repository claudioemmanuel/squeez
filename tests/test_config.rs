use squeez::config::Config;

#[test]
fn defaults_populated() {
    let c = Config::default();
    assert_eq!(c.max_lines, 120);
    assert_eq!(c.dedup_min, 2);
    assert!(c.enabled);
    assert!(c.show_header);
    assert_eq!(c.git_log_max_commits, 20);
    assert_eq!(c.docker_logs_max_lines, 100);
    assert!(c.bypass.contains(&"psql".to_string()));
}

#[test]
fn parses_flat_ini() {
    let ini = "max_lines = 100\ndedup_min = 5\nenabled = false\n";
    let c = Config::from_str(ini);
    assert_eq!(c.max_lines, 100);
    assert_eq!(c.dedup_min, 5);
    assert!(!c.enabled);
}

#[test]
fn ignores_comments_and_blanks() {
    let ini = "# comment\n\nmax_lines = 50\n";
    let c = Config::from_str(ini);
    assert_eq!(c.max_lines, 50);
}

#[test]
fn unknown_keys_silently_ignored() {
    let ini = "future_key = value\nmax_lines = 75\n";
    let c = Config::from_str(ini);
    assert_eq!(c.max_lines, 75);
}

#[test]
fn bypass_list_parsed() {
    let ini = "bypass = docker exec, psql, ssh\n";
    let c = Config::from_str(ini);
    assert!(c.bypass.contains(&"docker exec".to_string()));
    assert!(c.bypass.contains(&"psql".to_string()));
}

#[test]
fn is_bypassed_matches_prefix() {
    let c = Config::from_str("bypass = docker exec, psql\n");
    assert!(c.is_bypassed("docker exec -it foo bash"));
    assert!(c.is_bypassed("psql -U user mydb"));
    assert!(!c.is_bypassed("git status"));
}

#[test]
fn test_config_compact_threshold_default() {
    let c = squeez::config::Config::default();
    assert_eq!(c.compact_threshold_tokens, 90_000);
    assert_eq!(c.memory_retention_days, 30);
}

#[test]
fn test_config_compact_threshold_from_ini() {
    let c = squeez::config::Config::from_str(
        "compact_threshold_tokens = 80000\nmemory_retention_days = 14\n",
    );
    assert_eq!(c.compact_threshold_tokens, 80_000);
    assert_eq!(c.memory_retention_days, 14);
}
