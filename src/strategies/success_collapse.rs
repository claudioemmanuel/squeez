//! Collapses a benign, successful (exit 0, no error markers) run of a
//! low-signal command to a single `ok <cmd> (...)` line. Success is the
//! overwhelmingly common outcome for these commands and the full output
//! rarely carries decision-relevant detail once it's known to have
//! succeeded — the original is always still recoverable via the retrieve
//! stash, this only changes what prints inline.

use crate::context::factsheet;

const DEFAULT_ALLOWED_PREFIXES: &[&str] = &[
    "git push",
    "git pull",
    "git fetch",
    "git add",
    "git commit",
    "npm install",
    "npm ci",
    "npm update",
    "pnpm install",
    "pnpm add",
    "pnpm update",
    "yarn install",
    "yarn add",
    "yarn upgrade",
    "docker pull",
    "docker build",
    "wrangler deploy",
];

/// True when `cmd` matches a built-in low-signal prefix and isn't excluded
/// by `deny` (checked first, so a deny entry always wins).
pub fn is_eligible(cmd: &str, deny: &[String]) -> bool {
    let t = cmd.trim();
    if deny.iter().any(|d| !d.is_empty() && t.starts_with(d.as_str())) {
        return false;
    }
    DEFAULT_ALLOWED_PREFIXES.iter().any(|p| t.starts_with(p))
}

/// Builds the single collapsed line from the full captured stdout+stderr.
pub fn collapse(cmd: &str, combined: &str) -> String {
    let label = cmd.trim();
    let mut facts: Vec<String> = Vec::new();

    // Git ref updates ("<range>  local -> remote" or "* [new branch] local
    // -> remote") carry the single most useful fact for a push/pull/fetch:
    // which branches moved. Take the first, everything else is repetition
    // across multiple refspecs.
    if label.starts_with("git ") {
        if let Some(arrow) = git_ref_update(combined) {
            facts.push(arrow);
        }
    }

    for f in factsheet::extract(combined) {
        if facts.len() >= 3 {
            break;
        }
        if !facts.contains(&f) {
            facts.push(f);
        }
    }

    if facts.is_empty() {
        format!("ok {}", label)
    } else {
        format!("ok {} ({})", label, facts.join(", "))
    }
}

/// Finds a `... local -> remote` ref-update line and returns `"local → remote"`.
fn git_ref_update(text: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim();
        if let Some(idx) = t.find(" -> ") {
            let before = t[..idx].trim();
            let after = t[idx + 4..].trim();
            let local = before.rsplit(char::is_whitespace).next().unwrap_or(before);
            if !local.is_empty() && !after.is_empty() {
                return Some(format!("{} → {}", local, after));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eligible_prefixes_match() {
        assert!(is_eligible("git push origin main", &[]));
        assert!(is_eligible("npm install", &[]));
        assert!(is_eligible("docker build -t foo .", &[]));
        assert!(!is_eligible("git log", &[]));
        assert!(!is_eligible("rm -rf /", &[]));
    }

    #[test]
    fn deny_list_overrides_allow_list() {
        let deny = vec!["git push".to_string()];
        assert!(!is_eligible("git push origin main", &deny));
        assert!(is_eligible("git pull", &deny));
    }

    #[test]
    fn empty_deny_entries_are_ignored() {
        // A stray blank line in the config value must not match everything.
        let deny = vec!["".to_string()];
        assert!(is_eligible("git push origin main", &deny));
    }

    #[test]
    fn collapse_git_push_includes_branch_and_hash() {
        let combined = "To github.com:user/repo.git\n   a1b2c3d..e4f5g6h  main -> main\n";
        let out = collapse("git push", combined);
        assert!(out.starts_with("ok git push ("));
        assert!(out.contains("main → main"));
    }

    #[test]
    fn collapse_falls_back_to_bare_ok_with_no_facts() {
        let out = collapse("git add .", "");
        assert_eq!(out, "ok git add .");
    }

    #[test]
    fn collapse_docker_build_includes_image_tag_facts() {
        let combined = "Successfully built a1b2c3d4e5f6\nSuccessfully tagged myapp:1.2.3\n";
        let out = collapse("docker build -t myapp:1.2.3 .", combined);
        assert!(out.starts_with("ok docker build"));
        assert!(out.contains("a1b2c3d4e5f6") || out.contains("1.2.3"));
    }
}
