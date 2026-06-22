pub enum Keep {
    Head,
    Tail,
}

pub fn apply(lines: Vec<String>, limit: usize, keep: Keep) -> Vec<String> {
    if lines.len() <= limit {
        return lines;
    }
    let dropped = lines.len() - limit;
    let notice = format!("[... {} lines truncated]", dropped);
    match keep {
        Keep::Head => {
            let mut r: Vec<String> = lines.into_iter().take(limit).collect();
            r.push(notice);
            r
        }
        Keep::Tail => {
            let start = lines.len() - limit;
            let mut r = vec![notice];
            r.extend(lines.into_iter().skip(start));
            r
        }
    }
}

/// Error / failure signal words. A line containing any of these is almost
/// always what the caller is actually after, so it survives truncation.
const SIGNAL_WORDS: &[&str] = &[
    "error", "fail", "panic", "exception", "fatal", "traceback", "denied",
    "refused", "timeout", "cannot", "unable", "undefined", "unexpected",
    "warn", "abort", "missing", "not found", "stack",
];

/// Relevance-aware truncation (P3). Instead of blindly keeping the head, score
/// each line by how likely it is to matter — error/signal words, terms drawn
/// from the command itself ("the query"), and a small head/tail bias for
/// context — then keep the top `limit` lines **in their original order**.
///
/// The dropped lines are recoverable: `wrap` stashes the full original (P1),
/// so this can afford to be aggressive. Falls back to identity below `limit`.
pub fn apply_relevant(lines: Vec<String>, limit: usize, cmd: &str) -> Vec<String> {
    if lines.len() <= limit {
        return lines;
    }
    let total = lines.len();
    let terms = query_terms(cmd);

    // Rank line indices by score (desc), tie-broken by original position so the
    // selection is deterministic.
    let mut ranked: Vec<usize> = (0..total).collect();
    ranked.sort_by(|&a, &b| {
        score(&lines[b], &terms, b, total)
            .cmp(&score(&lines[a], &terms, a, total))
            .then(a.cmp(&b))
    });
    let mut keep: Vec<usize> = ranked.into_iter().take(limit).collect();
    keep.sort_unstable();

    let dropped = total - keep.len();
    let mut out = Vec::with_capacity(keep.len() + 1);
    out.push(format!(
        "[squeez: kept {} of {} lines by relevance — {} elided, squeez_retrieve for full]",
        keep.len(), total, dropped
    ));
    for i in keep {
        out.push(lines[i].clone());
    }
    out
}

/// Score a single line: higher = more worth keeping.
fn score(line: &str, terms: &[String], idx: usize, total: usize) -> i32 {
    let lower = line.to_lowercase();
    let mut s = 0;
    for w in SIGNAL_WORDS {
        if lower.contains(w) {
            s += 5;
        }
    }
    for t in terms {
        if lower.contains(t.as_str()) {
            s += 3;
        }
    }
    // Head lines are usually headers/context; the very last lines are the most
    // recent state. Give both a mild bias so a zero-signal output still reads
    // top-down.
    if idx < 3 {
        s += 4 - idx as i32;
    }
    if idx + 3 >= total {
        s += 1;
    }
    if line.trim().is_empty() {
        s -= 1;
    }
    s
}

/// Extract "query" terms from the command: meaningful words the caller typed
/// (skip the program name, flags, and very short tokens).
fn query_terms(cmd: &str) -> Vec<String> {
    cmd.split_whitespace()
        .skip(1) // the program name isn't a content term
        .filter(|t| !t.starts_with('-'))
        .map(|t| {
            t.trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | '(' | ')'))
                .to_lowercase()
        })
        .filter(|t| t.len() >= 3)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(n: usize, f: impl Fn(usize) -> String) -> Vec<String> {
        (0..n).map(f).collect()
    }

    #[test]
    fn relevant_keeps_error_lines_over_filler() {
        let mut lines = v(50, |i| format!("ok line {i} nothing special"));
        lines[40] = "ERROR: database connection refused".to_string();
        lines[41] = "panic: index out of bounds".to_string();
        let out = apply_relevant(lines, 10, "run-job");
        let body = out.join("\n");
        assert!(body.contains("database connection refused"), "error line must survive: {body}");
        assert!(body.contains("panic: index out of bounds"), "panic line must survive: {body}");
        // marker + 10 kept lines
        assert_eq!(out.len(), 11);
        assert!(out[0].contains("kept 10 of 50"));
    }

    #[test]
    fn relevant_keeps_command_query_terms() {
        let mut lines = v(40, |i| format!("noise {i}"));
        lines[25] = "found TODO in module auth".to_string();
        let out = apply_relevant(lines, 8, "grep TODO src/");
        assert!(out.join("\n").contains("found TODO in module auth"));
    }

    #[test]
    fn relevant_preserves_original_order() {
        let mut lines = v(30, |i| format!("filler {i}"));
        lines[5] = "error one".to_string();
        lines[20] = "error two".to_string();
        let out = apply_relevant(lines, 6, "cmd");
        let pos1 = out.iter().position(|l| l == "error one").unwrap();
        let pos2 = out.iter().position(|l| l == "error two").unwrap();
        assert!(pos1 < pos2, "kept lines must stay in original order");
    }

    #[test]
    fn relevant_is_identity_below_limit() {
        let lines = v(5, |i| format!("line {i}"));
        assert_eq!(apply_relevant(lines.clone(), 10, "cmd"), lines);
    }
}
