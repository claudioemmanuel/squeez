use std::io::Write;
use std::path::Path;

use crate::json_util::{escape_str, extract_str, extract_str_array, extract_u64, str_array};

pub struct Summary {
    pub date: String,
    pub duration_min: u64,
    pub tokens_saved: u64,
    pub files_touched: Vec<String>,
    pub files_committed: Vec<String>,
    pub test_summary: String,
    pub errors_resolved: Vec<String>,
    pub git_events: Vec<String>,
    pub ts: u64,
    /// Start of the validity window. Defaults to `ts` if not explicitly set.
    /// Summaries can be invalidated at a known timestamp so that `prune_old`
    /// ages them from `valid_to` rather than `ts`.
    pub valid_from: u64,
    /// End of the validity window. `0` means "still valid" (open-ended).
    /// When non-zero, this summary's facts are considered superseded as of
    /// that timestamp — `prune_old` then ages it from `valid_to` rather than
    /// from `ts`, so an invalidated summary doesn't outlive its retention.
    pub valid_to: u64,
    // ── Phase 1: structured memory fields ──────────────────────────────────
    /// Files read/globbed this session (up to 20, deduped).
    pub investigated: Vec<String>,
    /// Error snippets and git SHAs learned (up to 5, first 80 chars each).
    pub learned: Vec<String>,
    /// Successful test runs, builds, commits (up to 5).
    pub completed: Vec<String>,
    /// Failed tests and unresolved errors to revisit (up to 5).
    pub next_steps: Vec<String>,
    // ── Phase 7: token economy efficiency scores ──────────────────────────
    /// Compression ratio in basis points (0-10000 = 0-100%).
    pub compression_ratio_bp: u64,
    /// Tool choice efficiency in basis points.
    pub tool_choice_efficiency_bp: u64,
    /// Context reuse rate in basis points.
    pub context_reuse_rate_bp: u64,
    /// Budget conservation in basis points.
    pub budget_utilization_bp: u64,
    /// Weighted overall efficiency in basis points.
    pub efficiency_overall_bp: u64,
}

// ── Phase 3: cross-session search types ────────────────────────────────────

pub struct SearchResult {
    pub date: String,
    pub matched_field: String,
    pub matched_line: String,
}

pub struct FileHistoryResult {
    pub date: String,
    pub tokens_saved: u64,
    pub committed: bool,
}

/// Effective timestamp used by `prune_old` to age a summary out of the log.
/// Returns `valid_to` if set (non-zero), else `ts`. Free function so callers
/// don't need to import the trait — also makes the comparison cheap.
pub fn effective_ts(line: &str) -> u64 {
    let vt = extract_u64(line, "valid_to").unwrap_or(0);
    if vt > 0 {
        vt
    } else {
        extract_u64(line, "ts").unwrap_or(0)
    }
}

impl Summary {
    /// Mark this summary as superseded at the given timestamp. Idempotent.
    pub fn invalidate(&mut self, at: u64) {
        self.valid_to = at;
    }

    /// True iff `t` is within the summary's validity window.
    pub fn is_valid_at(&self, t: u64) -> bool {
        t >= self.valid_from && (self.valid_to == 0 || t < self.valid_to)
    }

    pub fn to_jsonl_line(&self) -> String {
        let valid_from = if self.valid_from == 0 {
            self.ts
        } else {
            self.valid_from
        };
        format!(
            "{{\"date\":\"{}\",\"duration_min\":{},\"tokens_saved\":{},\
\"files_touched\":{},\"files_committed\":{},\"test_summary\":\"{}\",\
\"errors_resolved\":{},\"git_events\":{},\"ts\":{},\
\"valid_from\":{},\"valid_to\":{},\
\"investigated\":{},\"learned\":{},\"completed\":{},\"next_steps\":{},\
\"compression_ratio_bp\":{},\"tool_choice_efficiency_bp\":{},\
\"context_reuse_rate_bp\":{},\"budget_utilization_bp\":{},\"efficiency_overall_bp\":{}}}",
            escape_str(&self.date),
            self.duration_min,
            self.tokens_saved,
            str_array(&self.files_touched),
            str_array(&self.files_committed),
            escape_str(&self.test_summary),
            str_array(&self.errors_resolved),
            str_array(&self.git_events),
            self.ts,
            valid_from,
            self.valid_to,
            str_array(&self.investigated),
            str_array(&self.learned),
            str_array(&self.completed),
            str_array(&self.next_steps),
            self.compression_ratio_bp,
            self.tool_choice_efficiency_bp,
            self.context_reuse_rate_bp,
            self.budget_utilization_bp,
            self.efficiency_overall_bp,
        )
    }

    pub fn from_jsonl_line(line: &str) -> Option<Self> {
        let ts = extract_u64(line, "ts").unwrap_or(0);
        // Temporal validity columns are optional for backwards compat with
        // summaries written by squeez < 0.3.
        let valid_from = extract_u64(line, "valid_from").unwrap_or(ts);
        let valid_to = extract_u64(line, "valid_to").unwrap_or(0);
        Some(Self {
            date: extract_str(line, "date")?,
            duration_min: extract_u64(line, "duration_min").unwrap_or(0),
            tokens_saved: extract_u64(line, "tokens_saved").unwrap_or(0),
            files_touched: extract_str_array(line, "files_touched"),
            files_committed: extract_str_array(line, "files_committed"),
            test_summary: extract_str(line, "test_summary").unwrap_or_default(),
            errors_resolved: extract_str_array(line, "errors_resolved"),
            git_events: extract_str_array(line, "git_events"),
            ts,
            valid_from,
            valid_to,
            // Phase 1 fields — optional; old JSONL entries load with empty Vecs.
            investigated: extract_str_array(line, "investigated"),
            learned: extract_str_array(line, "learned"),
            completed: extract_str_array(line, "completed"),
            next_steps: extract_str_array(line, "next_steps"),
            // Phase 7 fields — optional; old JSONL entries load with 0.
            compression_ratio_bp: extract_u64(line, "compression_ratio_bp").unwrap_or(0),
            tool_choice_efficiency_bp: extract_u64(line, "tool_choice_efficiency_bp").unwrap_or(0),
            context_reuse_rate_bp: extract_u64(line, "context_reuse_rate_bp").unwrap_or(0),
            budget_utilization_bp: extract_u64(line, "budget_utilization_bp").unwrap_or(0),
            efficiency_overall_bp: extract_u64(line, "efficiency_overall_bp").unwrap_or(0),
        })
    }

    pub fn display_line(&self) -> String {
        let n = self.files_touched.len();
        let files = format!("{} file{}", n, if n == 1 { "" } else { "s" });
        let commits = self.git_events.len();
        let git = if commits > 0 {
            format!(
                ", {} commit{}",
                commits,
                if commits == 1 { "" } else { "s" }
            )
        } else {
            String::new()
        };
        let pending = if !self.next_steps.is_empty() {
            format!(", {} pending", self.next_steps.len())
        } else {
            String::new()
        };
        format!("Prior session ({}): {}{}{}", self.date, files, git, pending)
    }
}

pub fn read_last_n(memory_dir: &Path, n: usize) -> Vec<Summary> {
    let content = match std::fs::read_to_string(memory_dir.join("summaries.jsonl")) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let mut summaries: Vec<Summary> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| Summary::from_jsonl_line(l))
        .collect();
    summaries.sort_by(|a, b| b.ts.cmp(&a.ts));
    summaries.truncate(n);
    summaries
}

pub fn write_summary(memory_dir: &Path, summary: &Summary) {
    let path = memory_dir.join("summaries.jsonl");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{}", summary.to_jsonl_line());
    }
}

/// Search summaries.jsonl for sessions where `query` appears in any text field.
/// Most-recent first; caps at 200 sessions to keep linear scan bounded.
pub fn search_history(memory_dir: &Path, query: &str, limit: usize) -> Vec<SearchResult> {
    let content = match std::fs::read_to_string(memory_dir.join("summaries.jsonl")) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let q = query.to_lowercase();
    let mut results: Vec<SearchResult> = Vec::new();
    let all_lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    let cap = all_lines.len().min(200);
    let start = all_lines.len().saturating_sub(cap);
    for line in all_lines[start..].iter().rev() {
        if results.len() >= limit {
            break;
        }
        let s = match Summary::from_jsonl_line(line) {
            Some(s) => s,
            None => continue,
        };
        let text_fields: &[(&str, &[String])] = &[
            ("errors_resolved", &s.errors_resolved),
            ("files_touched", &s.files_touched),
            ("investigated", &s.investigated),
            ("learned", &s.learned),
            ("completed", &s.completed),
            ("next_steps", &s.next_steps),
            ("test_summary", std::slice::from_ref(&s.test_summary)),
        ];
        for (field, items) in text_fields {
            if results.len() >= limit {
                break;
            }
            for item in *items {
                if item.to_lowercase().contains(&q) {
                    results.push(SearchResult {
                        date: s.date.clone(),
                        matched_field: field.to_string(),
                        matched_line: item.chars().take(120).collect(),
                    });
                    break; // one result per field per session
                }
            }
        }
    }
    results
}

/// Return sessions where `path_query` substring matches any files_touched entry.
/// Most-recent first; caps at 200 sessions.
pub fn file_history(memory_dir: &Path, path_query: &str, limit: usize) -> Vec<FileHistoryResult> {
    let content = match std::fs::read_to_string(memory_dir.join("summaries.jsonl")) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let q = path_query.to_lowercase();
    let mut results: Vec<FileHistoryResult> = Vec::new();
    let all_lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    let cap = all_lines.len().min(200);
    let start = all_lines.len().saturating_sub(cap);
    for line in all_lines[start..].iter().rev() {
        if results.len() >= limit {
            break;
        }
        let s = match Summary::from_jsonl_line(line) {
            Some(s) => s,
            None => continue,
        };
        if s.files_touched.iter().any(|f| f.to_lowercase().contains(&q)) {
            let committed = s.files_committed.iter().any(|f| f.to_lowercase().contains(&q));
            results.push(FileHistoryResult {
                date: s.date.clone(),
                tokens_saved: s.tokens_saved,
                committed,
            });
        }
    }
    results
}

/// Return a structured detail view of the JSONL event log for `date`.
/// Reads `{sessions_dir}/{date}*.jsonl`; truncates to ~2 KB if large.
pub fn session_detail(sessions_dir: &Path, date: &str) -> String {
    let safe: String = date
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(20)
        .collect();
    let dir = match std::fs::read_dir(sessions_dir) {
        Ok(d) => d,
        Err(_) => return "no sessions dir".to_string(),
    };
    let mut content = String::new();
    for entry in dir.flatten() {
        let fname = entry.file_name();
        let fname_str = fname.to_string_lossy();
        if fname_str.starts_with(&safe) && fname_str.ends_with(".jsonl") {
            content = std::fs::read_to_string(entry.path()).unwrap_or_default();
            break;
        }
    }
    if content.is_empty() {
        return format!("no session data for: {}", safe);
    }
    let mut total_calls: u32 = 0;
    let mut files: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut git_evts: Vec<String> = Vec::new();
    let mut test_sum = String::new();
    let mut total_in: u64 = 0;
    let mut total_out: u64 = 0;
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if extract_str(line, "type").as_deref() != Some("bash") {
            continue;
        }
        total_calls += 1;
        total_in += extract_u64(line, "in_tk").unwrap_or(0);
        total_out += extract_u64(line, "out_tk").unwrap_or(0);
        for f in extract_str_array(line, "files") {
            if !files.contains(&f) {
                files.push(f);
            }
        }
        for e in extract_str_array(line, "errors") {
            if !errors.contains(&e) {
                errors.push(e);
            }
        }
        for g in extract_str_array(line, "git") {
            if !git_evts.contains(&g) {
                git_evts.push(g);
            }
        }
        if let Some(ts) = extract_str(line, "test_summary") {
            if !ts.is_empty() {
                test_sum = ts;
            }
        }
    }
    let ratio = if total_in > 0 {
        total_in.saturating_sub(total_out) * 100 / total_in
    } else {
        0
    };
    let mut out = format!("session: {}\n", safe);
    out.push_str(&format!("total_calls: {}\n", total_calls));
    out.push_str(&format!("files_seen: {}\n", files.len()));
    out.push_str(&format!("errors: {}\n", errors.len()));
    out.push_str(&format!("git_events: {}\n", git_evts.len()));
    if !test_sum.is_empty() {
        out.push_str(&format!("test_summary: {}\n", test_sum));
    }
    if total_in > 0 {
        out.push_str(&format!(
            "compression_ratio: -{}% ({} → {} tok)\n",
            ratio, total_in, total_out
        ));
    }
    if out.len() > 2048 {
        let head: String = out.chars().take(1800).collect();
        let tail_src: String = out.chars().rev().take(200).collect::<String>().chars().rev().collect();
        format!("{}\n[squeez: truncated]\n{}", head, tail_src)
    } else {
        out
    }
}

pub fn prune_old(memory_dir: &Path, retention_days: u32) {
    let path = memory_dir.join("summaries.jsonl");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let cutoff = crate::session::unix_now().saturating_sub(retention_days as u64 * 86400);
    // Use effective_ts so invalidated summaries age from `valid_to` rather
    // than from `ts`. Lines with neither field present default to keep.
    let kept: Vec<&str> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter(|l| {
            let eff = effective_ts(l);
            eff == 0 || eff >= cutoff
        })
        .collect();
    let _ = std::fs::write(&path, kept.join("\n") + "\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(ts: u64, valid_to: u64) -> Summary {
        Summary {
            date: "2026-04-07".into(),
            duration_min: 0,
            tokens_saved: 0,
            files_touched: vec![],
            files_committed: vec![],
            test_summary: String::new(),
            errors_resolved: vec![],
            git_events: vec![],
            ts,
            valid_from: ts,
            valid_to,
            investigated: vec![],
            learned: vec![],
            completed: vec![],
            next_steps: vec![],
            compression_ratio_bp: 0,
            tool_choice_efficiency_bp: 0,
            context_reuse_rate_bp: 0,
            budget_utilization_bp: 0,
            efficiency_overall_bp: 0,
        }
    }

    #[test]
    fn invalidate_sets_valid_to() {
        let mut x = s(100, 0);
        assert!(x.is_valid_at(150));
        x.invalidate(200);
        assert!(x.is_valid_at(199));
        assert!(!x.is_valid_at(200));
        assert!(!x.is_valid_at(300));
    }

    #[test]
    fn jsonl_round_trip_with_temporal_columns() {
        let mut x = s(1000, 0);
        x.invalidate(2000);
        let line = x.to_jsonl_line();
        let back = Summary::from_jsonl_line(&line).unwrap();
        assert_eq!(back.ts, 1000);
        assert_eq!(back.valid_from, 1000);
        assert_eq!(back.valid_to, 2000);
    }

    #[test]
    fn legacy_jsonl_without_columns_loads_with_defaults() {
        // No valid_from / valid_to fields → defaults: valid_from=ts, valid_to=0
        let line = "{\"date\":\"2026-04-07\",\"duration_min\":0,\"tokens_saved\":0,\
\"files_touched\":[],\"files_committed\":[],\"test_summary\":\"\",\
\"errors_resolved\":[],\"git_events\":[],\"ts\":555}";
        let back = Summary::from_jsonl_line(line).unwrap();
        assert_eq!(back.ts, 555);
        assert_eq!(back.valid_from, 555);
        assert_eq!(back.valid_to, 0);
    }

    #[test]
    fn effective_ts_uses_valid_to_when_set() {
        let active_line = s(1000, 0).to_jsonl_line();
        let invalidated_line = s(1000, 5000).to_jsonl_line();
        assert_eq!(effective_ts(&active_line), 1000);
        assert_eq!(effective_ts(&invalidated_line), 5000);
    }
}
