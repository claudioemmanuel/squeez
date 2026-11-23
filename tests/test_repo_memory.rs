//! Session-start memory is scoped to the repository and shared across hosts.
use squeez::memory::read_for_repo;

fn line(date: &str, ts: u64, cwd: &str, next: &str) -> String {
    format!(
        r#"{{"date":"{date}","duration_min":1,"tokens_saved":0,"files_touched":[],"files_committed":[],"test_summary":"","errors_resolved":[],"git_events":[],"ts":{ts},"valid_from":{ts},"valid_to":0,"investigated":[],"learned":[],"completed":[],"next_steps":["{next}"],"cwd":"{cwd}"}}"#
    )
}

#[test]
fn only_this_repo_newest_first_with_other_hosts_labelled() {
    let root = std::env::temp_dir().join(format!("squeez-repomem-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (own, codex) = (root.join("claude/memory"), root.join("codex/memory"));
    std::fs::create_dir_all(&own).unwrap();
    std::fs::create_dir_all(&codex).unwrap();
    std::fs::write(
        own.join("summaries.jsonl"),
        [
            line("2026-11-20", 100, "/work/api", "fix flaky test"),
            line("2026-11-21", 200, "/work/web", "ECONNREFUSED in web"),
            line("2026-11-22", 300, "/work/api/", "token AKIAIOSFODNN7EXAMPLE leaked"),
        ]
        .join("\n")
            + "\n",
    )
    .unwrap();
    std::fs::write(codex.join("summaries.jsonl"), line("2026-11-23", 400, "/work/api", "migrate schema") + "\n").unwrap();

    let got = read_for_repo(&own, &[("codex".into(), codex.clone())], "/work/api", 3);
    let dates: Vec<&str> = got.iter().map(|s| s.date.as_str()).collect();
    assert_eq!(dates, ["2026-11-23 via codex", "2026-11-22", "2026-11-20"], "web session excluded, newest first");
    assert_eq!(got[0].next_steps, ["migrate schema"]);
    assert!(got[1].next_steps.is_empty(), "secret-looking step dropped: {:?}", got[1].next_steps);
    assert!(got[0].display_line().contains("(2026-11-23 via codex)"));

    // No cwd known: this host's last n, unchanged behaviour.
    let legacy = read_for_repo(&own, &[("codex".into(), codex)], "", 2);
    assert_eq!(legacy.len(), 2);
    assert_eq!(legacy[0].date, "2026-11-22");
}
