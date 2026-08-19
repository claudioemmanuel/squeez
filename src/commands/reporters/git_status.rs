//! `git status --porcelain=v1 -b` → a short human-readable summary.
//!
//! `git status` is the most frequent command in an agent session and its
//! default long form spends most of its bytes on instructions the model does
//! not need ("use git restore <file>… to discard changes"). The porcelain
//! form is compact, stable since git 1.7, and — unlike the long form —
//! machine-parseable without guessing.
//!
//! Two things this reporter refuses to do, both deliberate:
//!
//! 1. **It never collapses paths.** A count is not a path: an agent that has
//!    to re-run `git status` to learn *which* file changed has been made
//!    slower, not faster. Paths are listed verbatim; only the overflow past
//!    `MAX_PATHS` is summarized, and then the count says how many were cut.
//! 2. **It never emits raw porcelain codes.** `XY` status codes would make
//!    the model parse a format instead of reading a fact.
//!
//! Porcelain omits in-progress operation state (rebase, merge, bisect,
//! cherry-pick, revert), which is exactly the state an agent most needs. We
//! read the marker paths under `.git/` to recover it — no second `git
//! status` process, and no risk of two runs disagreeing.

use std::path::Path;

/// Cap on listed paths per section before the rest is summarized.
const MAX_PATHS: usize = 20;

/// How the parse went. Degrades in tiers rather than all-or-nothing: a shape
/// we only partly understand still yields its understood part, and a shape we
/// don't understand at all yields nothing so the caller falls back to the raw
/// output.
enum Parsed {
    /// Every line was recognized.
    Full(Summary),
    /// Recognized some lines; `unparsed` counts the rest.
    Degraded(Summary, usize),
    /// Nothing recognized — not porcelain output.
    Passthrough,
}

#[derive(Default)]
struct Summary {
    branch: Option<String>,
    staged: Vec<String>,
    unstaged: Vec<String>,
    untracked: Vec<String>,
    conflicted: Vec<String>,
}

impl Summary {
    fn is_clean(&self) -> bool {
        self.staged.is_empty()
            && self.unstaged.is_empty()
            && self.untracked.is_empty()
            && self.conflicted.is_empty()
    }
}

/// Entry point used by `reporters::detect_and_condense`. Returns `None` when
/// the command isn't a `git status` or the output isn't porcelain, so the
/// caller falls through to the generic filter untouched.
pub fn condense(cmd: &str, lines: &[String]) -> Option<Vec<String>> {
    if !is_git_status(cmd) {
        return None;
    }
    match parse(lines) {
        Parsed::Passthrough => None,
        Parsed::Full(summary) => Some(render(&summary, None)),
        Parsed::Degraded(summary, unparsed) => Some(render(&summary, Some(unparsed))),
    }
}

fn is_git_status(cmd: &str) -> bool {
    let mut it = cmd.split_whitespace();
    let first = it.next().unwrap_or("");
    let name = first.rsplit('/').next().unwrap_or(first);
    name == "git" && it.next() == Some("status")
}

/// The porcelain v1 status alphabet, plus the space that means "unchanged in
/// this column".
fn is_status_code(c: char) -> bool {
    matches!(c, ' ' | 'M' | 'A' | 'D' | 'R' | 'C' | 'U' | 'T' | '?' | '!')
}

fn parse(lines: &[String]) -> Parsed {
    let mut s = Summary::default();
    let mut unparsed = 0usize;
    let mut recognized = 0usize;

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        // `## main...origin/main [ahead 2]`
        if let Some(rest) = line.strip_prefix("## ") {
            s.branch = Some(rest.trim().to_string());
            recognized += 1;
            continue;
        }
        // Every other porcelain line is `XY <path>`, so it needs two status
        // columns and a separator.
        let bytes = line.as_bytes();
        if bytes.len() < 4 || bytes[2] != b' ' {
            unparsed += 1;
            continue;
        }
        let (x, y) = (bytes[0] as char, bytes[1] as char);
        // Both columns must come from porcelain's fixed alphabet. Without
        // this, the LONG form parses: "On branch main" has a space at index
        // 2, so it would be read as status `On` on a file called
        // "branch main". Misreading prose as a path is worse than not
        // compressing at all.
        if !is_status_code(x) || !is_status_code(y) {
            unparsed += 1;
            continue;
        }
        let path = line[3..].trim().to_string();
        if path.is_empty() {
            unparsed += 1;
            continue;
        }
        recognized += 1;
        match (x, y) {
            ('?', '?') => s.untracked.push(path),
            // Unmerged: either side 'U', or both sides the same A/D.
            ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D') => s.conflicted.push(path),
            _ => {
                if x != ' ' {
                    s.staged.push(path.clone());
                }
                if y != ' ' {
                    s.unstaged.push(path);
                }
            }
        }
    }

    if recognized == 0 {
        return Parsed::Passthrough;
    }
    if unparsed == 0 {
        Parsed::Full(s)
    } else {
        Parsed::Degraded(s, unparsed)
    }
}

fn render(s: &Summary, unparsed: Option<usize>) -> Vec<String> {
    let mut out = Vec::new();

    let mut head = match &s.branch {
        Some(b) => format!("branch {}", b),
        None => "branch unknown".to_string(),
    };
    if let Some(op) = in_progress_operation(Path::new(".git")) {
        // The one fact porcelain drops entirely, and the one an agent most
        // needs before it decides what to do next.
        head.push_str(&format!(" — {} IN PROGRESS", op));
    }
    out.push(head);

    if s.is_clean() {
        out.push("clean working tree".to_string());
    } else {
        push_section(&mut out, "conflicted", &s.conflicted);
        push_section(&mut out, "staged", &s.staged);
        push_section(&mut out, "unstaged", &s.unstaged);
        push_section(&mut out, "untracked", &s.untracked);
    }

    if let Some(n) = unparsed {
        out.push(format!(
            "[squeez: {} line(s) of this status were not recognized as porcelain \
             — rerun `git status` if something looks missing]",
            n
        ));
    }
    out
}

fn push_section(out: &mut Vec<String>, label: &str, paths: &[String]) {
    if paths.is_empty() {
        return;
    }
    out.push(format!("{} ({}):", label, paths.len()));
    for p in paths.iter().take(MAX_PATHS) {
        out.push(format!("  {}", p));
    }
    if paths.len() > MAX_PATHS {
        out.push(format!("  … {} more", paths.len() - MAX_PATHS));
    }
}

/// The in-progress operation, read from `.git/` marker paths. Ordered most
/// specific first: a rebase carries a `MERGE_HEAD`-like state of its own, so
/// checking merge first would mislabel it.
fn in_progress_operation(git_dir: &Path) -> Option<&'static str> {
    const MARKERS: &[(&str, &str)] = &[
        ("rebase-merge", "rebase"),
        ("rebase-apply", "rebase"),
        ("CHERRY_PICK_HEAD", "cherry-pick"),
        ("REVERT_HEAD", "revert"),
        ("BISECT_LOG", "bisect"),
        ("MERGE_HEAD", "merge"),
    ];
    MARKERS
        .iter()
        .find(|(marker, _)| git_dir.join(marker).exists())
        .map(|(_, label)| *label)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(String::from).collect()
    }

    #[test]
    fn ignores_commands_that_are_not_git_status() {
        assert!(condense("git log --oneline", &lines("## main")).is_none());
        assert!(condense("gitk", &lines("## main")).is_none());
    }

    #[test]
    fn clean_tree_reports_branch_and_clean() {
        let out = condense("git status", &lines("## main...origin/main")).unwrap();
        assert_eq!(out[0], "branch main...origin/main");
        assert!(out.contains(&"clean working tree".to_string()));
    }

    #[test]
    fn splits_staged_unstaged_and_untracked_keeping_exact_paths() {
        let out = condense(
            "git status",
            &lines("## feat/x\nM  src/a.rs\n M src/b.rs\nMM src/c.rs\n?? notes.txt"),
        )
        .unwrap()
        .join("\n");
        // A path appearing in both columns must appear under both sections —
        // collapsing it would hide half the state.
        assert!(out.contains("staged (2):"), "{out}");
        assert!(out.contains("  src/a.rs"), "{out}");
        assert!(out.contains("  src/c.rs"), "{out}");
        assert!(out.contains("unstaged (2):"), "{out}");
        assert!(out.contains("  src/b.rs"), "{out}");
        assert!(out.contains("untracked (1):"), "{out}");
        assert!(out.contains("  notes.txt"), "{out}");
    }

    #[test]
    fn conflicts_get_their_own_section() {
        let out = condense("git status", &lines("## main\nUU src/merge.rs\nAA src/both.rs"))
            .unwrap()
            .join("\n");
        assert!(out.contains("conflicted (2):"), "{out}");
    }

    #[test]
    fn never_emits_raw_porcelain_codes() {
        let out = condense("git status", &lines("## main\nM  src/a.rs\n?? b.txt"))
            .unwrap()
            .join("\n");
        for token in ["M  ", "?? "] {
            assert!(!out.contains(token), "raw porcelain code leaked: {out}");
        }
    }

    #[test]
    fn unrecognized_output_falls_through_to_the_raw_filter() {
        // The long form is not porcelain — the caller must keep its own path.
        let long = "On branch main\nnothing to commit, working tree clean";
        assert!(condense("git status", &lines(long)).is_none());
    }

    #[test]
    fn partial_parse_degrades_and_says_so() {
        let out = condense("git status", &lines("## main\nM  src/a.rs\nxx"))
            .unwrap()
            .join("\n");
        assert!(out.contains("not recognized as porcelain"), "{out}");
    }

    #[test]
    fn long_path_lists_are_capped_with_an_explicit_count() {
        let mut input = String::from("## main\n");
        for i in 0..25 {
            input.push_str(&format!("?? file{}.txt\n", i));
        }
        let out = condense("git status", &lines(&input)).unwrap().join("\n");
        assert!(out.contains("untracked (25):"), "{out}");
        assert!(out.contains("… 5 more"), "{out}");
    }

    #[test]
    fn in_progress_operation_reads_the_git_dir_markers() {
        let dir = std::env::temp_dir().join(format!("squeez-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("rebase-merge")).unwrap();
        assert_eq!(in_progress_operation(&dir), Some("rebase"));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(in_progress_operation(&dir), None);
    }
}
