//! `squeez discover --suggest "<cmd>"` — propose a filter-DSL rule for a
//! command squeez compresses poorly, from that command's real output.
//!
//! `squeez discover` names the weak commands; until now the user had to write
//! the rule by hand. Samples come from the retrieve stash (outputs the
//! command's handler produced, via `blobs/origins.tsv`) or from stdin
//! (`cmd 2>&1 | squeez discover --suggest "cmd" -`). Lines repeating under
//! one key (their first two words, digits masked) are the noise; each repeated group becomes a
//! `strip_lines_containing` on the literal text its lines share. A literal is
//! rejected if any error-looking line in the samples contains it, so the rule
//! cannot swallow a failure. The rule is measured on the samples through the
//! real DSL engine, and written to the user's filters.ini only with --apply.

use std::collections::HashMap;

use crate::filter_dsl::{self, FilterDef, Stage};

/// Lines sharing a template this many times are noise.
const MIN_REPEAT: usize = 3;
/// Shortest literal worth matching on.
const MIN_LITERAL: usize = 8;
const MAX_STRIPS: usize = 8;

fn is_error_like(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    ["error", "fail", "panic", "exception", "fatal", "warn", "denied", "traceback"]
        .iter()
        .any(|k| l.contains(k))
}

/// Lines of one kind share their first two words once digits are masked
/// (`Downloading https://…/a-1.jar` and `…/a-2.jar`).
fn group_key(line: &str) -> String {
    line.split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .map(|c| if c.is_ascii_digit() { '#' } else { c })
        .collect()
}

/// Longest common prefix of `lines`, cut back to a word boundary.
fn common_literal(lines: &[&str]) -> String {
    let first = lines[0].trim();
    let mut end = first.len();
    for l in &lines[1..] {
        let l = l.trim();
        end = first
            .char_indices()
            .zip(l.chars())
            .take_while(|((_, a), b)| a == b)
            .map(|((i, a), _)| i + a.len_utf8())
            .last()
            .unwrap_or(0)
            .min(end);
    }
    let lit = &first[..end];
    // Do not end mid-word: a version or count would follow.
    let cut = if end < first.len() {
        lit.rfind([' ', ':', '=', '/', '(']).map(|i| i + 1).unwrap_or(0)
    } else {
        end
    };
    lit[..cut].trim_end().to_string()
}

/// The suggested rule, or `None` when the samples hold no safe repeated noise.
pub fn suggest(cmd: &str, samples: &[String]) -> Option<FilterDef> {
    let all: Vec<&str> = samples.iter().flat_map(|s| s.lines()).collect();
    let errors: Vec<&str> = all.iter().copied().filter(|l| is_error_like(l)).collect();
    let mut groups: HashMap<String, Vec<&str>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for l in all.iter().copied().filter(|l| !l.trim().is_empty() && !is_error_like(l)) {
        let t = group_key(l);
        if !groups.contains_key(&t) {
            order.push(t.clone());
        }
        groups.entry(t).or_default().push(l);
    }
    let mut strips: Vec<(usize, String)> = Vec::new();
    for t in &order {
        let lines = &groups[t];
        if lines.len() < MIN_REPEAT {
            continue;
        }
        let lit = common_literal(lines);
        let safe = lit.chars().count() >= MIN_LITERAL
            && !lit.contains('"')
            && !errors.iter().any(|e| e.contains(lit.as_str()))
            && !strips.iter().any(|(_, s)| lit.contains(s.as_str()));
        if safe {
            strips.push((lines.len(), lit));
        }
    }
    if strips.is_empty() {
        return None;
    }
    strips.sort_by(|a, b| b.0.cmp(&a.0));
    strips.truncate(MAX_STRIPS);
    let mut stages = vec![Stage::StripAnsi];
    stages.extend(strips.into_iter().map(|(_, s)| Stage::StripLinesContaining(s)));
    let name = if cmd.contains(' ') { cmd.to_string() } else { format!("{cmd} ") };
    let mut def = FilterDef { name, stages, tests: Vec::new() };
    // Fixture: a noise line, plus a kept line (an error if the samples had one).
    let noise = all.iter().copied().find(|l| {
        def.stages.iter().any(|s| matches!(s, Stage::StripLinesContaining(p) if l.contains(p.as_str())))
    })?;
    let kept = errors.first().copied().or_else(|| {
        all.iter().copied().find(|l| filter_dsl::apply(&def, vec![l.to_string()]).len() == 1 && !l.trim().is_empty())
    })?;
    let input = format!("{noise}\n{kept}");
    let expect = filter_dsl::apply(&def, input.lines().map(String::from).collect()).join("\n");
    def.tests.push((input, expect));
    Some(def)
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\n', "\\n"))
}

/// The rule as filters.ini text.
pub fn render(def: &FilterDef, n_samples: usize) -> String {
    let mut out = format!(
        "# suggested by `squeez discover --suggest` from {n_samples} sample(s) — review before relying on it\n[filter {}]\n",
        quote(&def.name)
    );
    for s in &def.stages {
        match s {
            Stage::StripAnsi => out.push_str("strip_ansi = true\n"),
            Stage::StripLinesContaining(p) => out.push_str(&format!("strip_lines_containing = {}\n", quote(p))),
            _ => {}
        }
    }
    for (i, e) in &def.tests {
        out.push_str(&format!("test_input = {}\ntest_expect = {}\n", quote(i), quote(e)));
    }
    out
}

/// `(tokens before, tokens after)` of the rule over the samples.
pub fn measure(def: &FilterDef, samples: &[String]) -> (usize, usize) {
    samples.iter().fold((0, 0), |(b, a), s| {
        let after = filter_dsl::apply(def, s.lines().map(String::from).collect()).join("\n");
        (b + crate::tokens::estimate(s), a + crate::tokens::estimate(&after))
    })
}

/// Stashed outputs produced by `handler`, newest first, at most `n`.
fn stashed_samples(handler: &str, n: usize) -> Vec<String> {
    let dir = crate::context::retrieve::blobs_dir();
    let Ok(origins) = std::fs::read_to_string(dir.join("origins.tsv")) else {
        return Vec::new();
    };
    let mut ids: Vec<&str> = origins
        .lines()
        .rev()
        .filter_map(|l| l.split_once('\t').filter(|(_, h)| *h == handler).map(|(id, _)| id))
        .collect();
    ids.dedup();
    ids.into_iter().filter_map(crate::context::retrieve::retrieve).take(n).collect()
}

/// CLI: `squeez discover --suggest "<cmd>" [-] [--apply]`.
pub fn run(args: &[String]) -> i32 {
    let Some(cmd) = args.iter().skip_while(|a| *a != "--suggest").nth(1).filter(|c| !c.starts_with('-')) else {
        eprintln!("usage: squeez discover --suggest \"<cmd>\" [-] [--apply]");
        return 2;
    };
    let mut samples = Vec::new();
    if args.iter().any(|a| a == "-") {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut s);
        if !s.trim().is_empty() {
            samples.push(s);
        }
    }
    let handler = crate::filter::dispatch_name(cmd);
    let handler = if handler.is_empty() { cmd.split_whitespace().next().unwrap_or("").to_string() } else { handler };
    samples.extend(stashed_samples(&handler, 12));
    if samples.is_empty() {
        eprintln!(
            "squeez discover: no stashed output for '{handler}' yet — pipe one in: `{cmd} 2>&1 | squeez discover --suggest \"{cmd}\" -`"
        );
        return 1;
    }
    let Some(def) = suggest(cmd, &samples) else {
        println!("squeez discover: no safe repeated noise found in {} sample(s) of '{cmd}'.", samples.len());
        return 0;
    };
    let (before, after) = measure(&def, &samples);
    let text = render(&def, samples.len());
    println!("{text}");
    println!(
        "# on these samples: {before} → {after} tokens ({}% less)",
        (before.saturating_sub(after) * 100).checked_div(before).unwrap_or(0)
    );
    if !args.iter().any(|a| a == "--apply") {
        println!("# not written — rerun with --apply to append it to {}", filter_dsl::user_filters_path().display());
        return 0;
    }
    let path = filter_dsl::user_filters_path();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if filter_dsl::parse(&existing).iter().any(|d| d.name == def.name) {
        eprintln!("squeez discover: {} already has a rule for {:?} — not overwritten", path.display(), def.name);
        return 1;
    }
    let sep = if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" };
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    match std::fs::write(&path, format!("{existing}{sep}\n{text}")) {
        Ok(()) => {
            println!("# written to {}", path.display());
            0
        }
        Err(e) => {
            eprintln!("squeez discover: cannot write {}: {e}", path.display());
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> String {
        let mut s = String::new();
        for i in 0..20 {
            s.push_str(&format!("INFO: Analyzed target //pkg/mod{i}:lib (12{i} packages loaded)\n"));
        }
        for i in 0..5 {
            s.push_str(&format!("Downloading https://repo.example/artifact-{i}.jar\n"));
        }
        s.push_str("ERROR: //pkg/mod3:lib failed to build\n");
        s.push_str("Build completed, 1 test FAILED\n");
        s
    }

    #[test]
    fn repeated_noise_becomes_strips_and_errors_survive() {
        let def = suggest("bazel test", &[sample()]).expect("rule");
        let strips: Vec<&str> = def
            .stages
            .iter()
            .filter_map(|s| match s {
                Stage::StripLinesContaining(p) => Some(p.as_str()),
                _ => None,
            })
            .collect();
        assert!(strips.iter().any(|s| s.starts_with("INFO: Analyzed target")), "{strips:?}");
        assert!(strips.iter().any(|s| s.starts_with("Downloading https:")), "{strips:?}");
        let out = filter_dsl::apply(&def, sample().lines().map(String::from).collect());
        assert_eq!(out, vec!["ERROR: //pkg/mod3:lib failed to build", "Build completed, 1 test FAILED"]);
        let (b, a) = measure(&def, &[sample()]);
        assert!(a * 5 < b, "{b} -> {a}");
        // The rendered rule parses back and its own fixture passes.
        let parsed = filter_dsl::parse(&render(&def, 1));
        assert_eq!(parsed.len(), 1);
        assert!(filter_dsl::run_tests(&parsed).iter().all(|r| r.passed == 1 && r.failed.is_empty()), "{}", render(&def, 1));
    }

    #[test]
    fn a_literal_seen_in_an_error_is_never_stripped() {
        let mut s = String::new();
        for i in 0..5 {
            s.push_str(&format!("compiling module number {i}\n"));
        }
        s.push_str("compiling module number 9 error: boom\n");
        assert!(suggest("tool", &[s]).is_none());
        assert!(suggest("tool", &["one\ntwo\n".into()]).is_none(), "no repeats");
        assert_eq!(common_literal(&["abc def 1", "abc def 22"]), "abc def");
    }
}
