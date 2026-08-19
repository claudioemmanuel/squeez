//! Gates on the shipped filter-DSL rule pack (`assets/filters_builtin.ini`).
//!
//! The pack outranks every handler in `filter.rs`, so a careless rule is not
//! a missed optimization — it hijacks a command that already compressed well.
//! These tests are the gate that keeps that out of a release.

use squeez::filter_dsl;

#[test]
fn every_builtin_rule_carries_a_passing_self_test() {
    let defs = filter_dsl::builtin_defs();
    assert!(!defs.is_empty(), "the shipped pack parsed to zero rules");

    let untested: Vec<&str> = defs
        .iter()
        .filter(|d| d.tests.is_empty())
        .map(|d| d.name.as_str())
        .collect();
    assert!(untested.is_empty(), "built-in rules without a self-test: {untested:?}");

    let reports = filter_dsl::run_tests(&defs);
    let failures: Vec<String> = reports
        .iter()
        .filter(|r| !r.failed.is_empty())
        .map(|r| format!("{} ({} failing)", r.filter_name, r.failed.len()))
        .collect();
    assert!(failures.is_empty(), "failing built-in self-tests: {failures:?}");
}

#[test]
fn no_builtin_rule_hijacks_a_command_with_a_dedicated_handler() {
    // A DSL rule wins over the dispatch table, so a rule whose name routes to
    // a real handler would silently replace it.
    for def in filter_dsl::builtin_defs() {
        let handler = squeez::filter::handler_name(&def.name);
        assert_eq!(
            handler, "generic",
            "built-in rule {:?} shadows the {handler:?} handler — either rename \
             the rule or drop it; the handler already compresses that command",
            def.name
        );
    }
}

#[test]
fn builtin_rules_do_not_prefix_match_handled_commands() {
    // The classic trap: `[filter "ps"]` matches `psql` by prefix. Every rule
    // name must be specific enough (trailing space or subcommand) that these
    // commands keep reaching their handlers.
    const HANDLED: &[&str] = &[
        "psql -c 'select 1'",
        "git status",
        "docker ps",
        "npm install",
        "cargo build",
        "pytest tests/",
        "tsc --noEmit",
        "kubectl get pods",
        "terraform plan",
        "curl https://example.com",
        "make build",
        "gradle assemble",
        "jq '.items' out.json",
        "grep -rn TODO src/",
        "find . -name '*.rs'",
        "ps aux",
        "du -sh .",
    ];
    for cmd in HANDLED {
        let hit = filter_dsl::find_for_command(cmd, true);
        assert!(
            hit.is_none(),
            "{cmd:?} is captured by built-in rule {:?}, bypassing its handler",
            hit.map(|d| d.name).unwrap_or_default()
        );
    }
}

#[test]
fn a_user_rule_shadows_a_same_named_builtin() {
    // Precedence contract: built-in loads first precisely so later layers win
    // the `max_by_key` tie-break. Verified here at the merge level.
    let builtin = filter_dsl::builtin_defs();
    let name = builtin[0].name.clone();
    let user = filter_dsl::parse(&format!(
        "[filter {name:?}]\non_empty = \"from the user layer\"\n"
    ));
    let mut merged = builtin;
    merged.extend(user);
    let winner = filter_dsl::find_in(&merged, &format!("{name} --flag"));
    assert_eq!(
        winner.map(|d| d.stages),
        Some(vec![filter_dsl::Stage::OnEmpty("from the user layer".to_string())]),
        "a same-named user rule must shadow the built-in"
    );
}
