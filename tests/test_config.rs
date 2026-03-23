use squeez::config::Config;

#[test]
fn defaults_populated() {
    let c = Config::default();
    assert_eq!(c.max_lines, 200);
    assert_eq!(c.dedup_min, 3);
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
