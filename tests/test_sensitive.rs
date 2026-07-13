// Integration tests for the sensitive-content guard (Epic E7):
// context::sensitive::is_sensitive. Covers every detection class plus the
// acceptance-criterion false-positive scan over bench/fixtures/.

use squeez::context::sensitive::is_sensitive;
use std::path::Path;

#[test]
fn detects_private_key_block() {
    let s = "-----BEGIN RSA PRIVATE KEY-----\nMIIEow...\n-----END RSA PRIVATE KEY-----";
    assert_eq!(is_sensitive(s), Some("private-key-block"));
}

#[test]
fn detects_aws_access_key() {
    assert_eq!(
        is_sensitive("aws_access_key_id=AKIAIOSFODNN7EXAMPLE"),
        Some("aws-access-key")
    );
}

#[test]
fn detects_github_classic_token() {
    assert_eq!(
        is_sensitive("git remote set-url origin https://ghp_abcdefghijklmnopqrstuvwxyzABCDEF@github.com/x/y"),
        Some("github-token")
    );
}

#[test]
fn detects_github_fine_grained_token() {
    assert_eq!(
        is_sensitive("github_pat_11ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"),
        Some("github-token")
    );
}

#[test]
fn detects_slack_tokens() {
    // Not shaped like Slack's real numeric-group token format -- a
    // fixture for our own loose prefix check, not a real credential.
    assert_eq!(
        is_sensitive("xoxb-fixturenotarealtoken-testdataonly-nnnn"),
        Some("slack-token")
    );
    assert_eq!(
        is_sensitive("xoxp-fixturenotarealtoken-testdataonly-nnnn"),
        Some("slack-token")
    );
}

#[test]
fn detects_openai_style_secret_key() {
    assert_eq!(
        is_sensitive("export OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz123456"),
        Some("api-secret-key")
    );
}

#[test]
fn detects_bearer_token_header() {
    assert_eq!(
        is_sensitive("> Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.abcdefg"),
        Some("bearer-token")
    );
}

#[test]
fn detects_aws_secret_access_key_assignment() {
    assert_eq!(
        is_sensitive("aws_secret_access_key = wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"),
        Some("aws-secret-line")
    );
    assert_eq!(
        is_sensitive("AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"),
        Some("aws-secret-line")
    );
}

#[test]
fn detects_dotenv_bulk_secrets() {
    let body = "APP_NAME=demo\nDB_PASSWORD=hunter2\nSTRIPE_SECRET=sk_live_xxx\n\
                API_TOKEN=abc123\nDEBUG=true\n";
    assert_eq!(is_sensitive(body), Some("dotenv-bulk-secrets"));
}

#[test]
fn single_dotenv_secret_line_is_not_flagged() {
    // Below the bulk threshold (needs >= 3 matching lines) and no other
    // class fires either.
    assert_eq!(
        is_sensitive("DB_PASSWORD=hunter2\nDEBUG=true\nPORT=8080\nHOST=localhost"),
        None
    );
}

#[test]
fn ordinary_content_is_clean() {
    let samples = [
        "PATH=/usr/bin:/bin\nHOME=/Users/dev\nSHELL=/bin/zsh\nLANG=C.UTF-8",
        "this is a desk-based risk-averse task-oriented plan",
        "error: cannot find module 'foo' — check your imports",
        "commit abc1234 fixed the ticket PROJ-1482 in v1.34.6",
    ];
    for s in samples {
        assert_eq!(is_sensitive(s), None, "false positive on: {s}");
    }
}

/// Acceptance criterion: running `is_sensitive` over every file in
/// bench/fixtures/ must produce zero hits. These are real captured command
/// outputs (git, npm, cargo, docker, kubectl, env dumps, …) — none of them
/// legitimately contain a credential shape, so any hit here is a false
/// positive in the scanner that must be fixed, not silenced.
#[test]
fn no_false_positives_on_bench_fixtures() {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("bench/fixtures");
    let entries = std::fs::read_dir(&fixtures_dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", fixtures_dir.display()));

    let mut checked = 0usize;
    for entry in entries {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue; // skip non-UTF8 fixtures, if any
        };
        checked += 1;
        assert_eq!(
            is_sensitive(&content),
            None,
            "false positive in fixture {}",
            path.display()
        );
    }
    assert!(checked > 10, "expected to scan a meaningful number of fixtures, got {checked}");
}
