use squeez::commands::focus::{as_str, banner_suffix, from_str, text, text_with_lang, Focus};
use squeez::commands::init::banner_body;
use squeez::config::Config;
use squeez::memory::Summary;

#[test]
fn default_is_off() {
    assert_eq!(Focus::default(), Focus::Off);
    assert_eq!(Config::default().focus, Focus::Off);
}

#[test]
fn from_str_round_trip() {
    for f in [Focus::Off, Focus::Adhd] {
        assert_eq!(from_str(as_str(f)), f);
    }
}

#[test]
fn from_str_accepts_aliases_and_case() {
    assert_eq!(from_str("ADHD"), Focus::Adhd);
    assert_eq!(from_str(" adhd "), Focus::Adhd);
    assert_eq!(from_str("on"), Focus::Adhd);
    assert_eq!(from_str("true"), Focus::Adhd);
}

#[test]
fn from_str_unknown_is_off() {
    assert_eq!(from_str("focused"), Focus::Off);
    assert_eq!(from_str(""), Focus::Off);
}

#[test]
fn off_emits_nothing() {
    assert_eq!(text(Focus::Off), "");
    assert_eq!(text_with_lang(Focus::Off, "pt-BR"), "");
    assert_eq!(banner_suffix(Focus::Off), "");
}

#[test]
fn adhd_text_is_lang_selected() {
    let en = text_with_lang(Focus::Adhd, "en");
    let pt = text_with_lang(Focus::Adhd, "pt-BR");
    assert!(en.contains("Lead with next action"));
    assert!(pt.contains("próxima ação"));
    assert_ne!(en, pt);
    // Unknown language falls back to English rather than emitting nothing.
    assert_eq!(text_with_lang(Focus::Adhd, "fr"), en);
    assert_eq!(banner_suffix(Focus::Adhd), " | Focus: adhd");
}

#[test]
fn adhd_text_carries_all_ten_rules() {
    let en = text_with_lang(Focus::Adhd, "en");
    let pt = text_with_lang(Focus::Adhd, "pt-BR");
    for n in 1..=10 {
        assert!(en.contains(&format!("\n{}. ", n)), "EN missing rule {}", n);
        assert!(pt.contains(&format!("\n{}. ", n)), "pt-BR missing rule {}", n);
    }
}

fn summary_with_steps(date: &str, steps: &[&str]) -> Summary {
    Summary {
        date: date.to_string(),
        duration_min: 10,
        tokens_saved: 1_000,
        files_touched: vec!["src/commands/focus.rs".to_string()],
        files_committed: vec![],
        test_summary: String::new(),
        errors_resolved: vec![],
        git_events: vec![],
        ts: 1_774_569_600,
        valid_from: 1_774_569_600,
        valid_to: 0,
        investigated: vec![],
        learned: vec![],
        completed: vec![],
        next_steps: steps.iter().map(|s| s.to_string()).collect(),
        compression_ratio_bp: 0,
        tool_choice_efficiency_bp: 0,
        context_reuse_rate_bp: 0,
        budget_utilization_bp: 0,
        efficiency_overall_bp: 0,
    }
}

#[test]
fn banner_off_keeps_stats_first() {
    let cfg = Config::default();
    let summaries = vec![summary_with_steps("2026-07-22", &["ship focus mode"])];
    let lines = banner_body(&cfg, &summaries, "STATS".to_string(), None);
    assert_eq!(lines[0], "STATS");
    assert!(lines.iter().any(|l| l.contains("Next steps: ship focus mode")));
    assert!(!lines.iter().any(|l| l.starts_with("Next: ")));
}

#[test]
fn banner_adhd_leads_with_next_action() {
    let cfg = Config::from_str("focus = adhd\n");
    let summaries = vec![summary_with_steps(
        "2026-07-22",
        &["ship focus mode", "update changelog", "cut release"],
    )];
    let lines = banner_body(&cfg, &summaries, "STATS".to_string(), None);
    assert_eq!(lines[0], "Next: ship focus mode");
    assert_eq!(lines[1], "  2. update changelog");
    assert_eq!(lines[2], "  3. cut release");
    // Stats are state, not an action — they sink to the bottom.
    assert_eq!(lines.last().unwrap(), "STATS");
}

#[test]
fn banner_adhd_without_steps_still_names_an_action() {
    let cfg = Config::from_str("focus = adhd\n");
    let lines = banner_body(&cfg, &[], "STATS".to_string(), None);
    assert_eq!(lines[0], "Next: no pending step — name what to work on.");
}

#[test]
fn banner_adhd_keeps_degraded_warning_high() {
    let cfg = Config::from_str("focus = adhd\n");
    let summaries = vec![summary_with_steps("2026-07-22", &["a", "b"])];
    let lines = banner_body(
        &cfg,
        &summaries,
        "STATS".to_string(),
        Some("[squeez doctor: pipeline degraded]".to_string()),
    );
    assert_eq!(lines[0], "Next: a");
    assert_eq!(lines[1], "[squeez doctor: pipeline degraded]");
}

#[test]
fn advisories_are_capped_only_under_focus() {
    let many: Vec<String> = (1..=8).map(|i| format!("[squeez: w{}]", i)).collect();

    let off = squeez::commands::focus::cap_advisories(many.clone(), Focus::Off);
    assert_eq!(off.len(), 8, "focus off must not cap advisories");

    let on = squeez::commands::focus::cap_advisories(many.clone(), Focus::Adhd);
    assert_eq!(on.len(), 6);
    assert_eq!(on[4], "[squeez: w5]");
    assert_eq!(on[5], "[squeez: +3 more advisories suppressed]");

    // At or below the cap nothing is added.
    let few: Vec<String> = many.into_iter().take(5).collect();
    assert_eq!(squeez::commands::focus::cap_advisories(few, Focus::Adhd).len(), 5);
}
