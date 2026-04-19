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
    if n == 0 {
        return Vec::new();
    }
    let path = memory_dir.join("summaries.jsonl");
    use std::io::{Read, Seek, SeekFrom};
    let mut file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let file_len = match file.seek(SeekFrom::End(0)) {
        Ok(l) => l,
        Err(_) => return Vec::new(),
    };
    if file_len == 0 {
        return Vec::new();
    }

    const CHUNK: u64 = 8192;
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    let mut pos = file_len;
    let mut newline_count = 0usize;

    while pos > 0 {
        let read_from = pos.saturating_sub(CHUNK);
        let read_size = (pos - read_from) as usize;
        if file.seek(SeekFrom::Start(read_from)).is_err() {
            break;
        }
        let mut chunk = vec![0u8; read_size];
        if file.read_exact(&mut chunk).is_err() {
            break;
        }
        newline_count += chunk.iter().filter(|&&b| b == b'\n').count();
        chunks.push(chunk);
        pos = read_from;
        if newline_count > n {
            break;
        }
    }

    chunks.reverse();
    let buf: Vec<u8> = chunks.into_iter().flatten().collect();
    let text = String::from_utf8_lossy(&buf);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(n);
    let mut summaries: Vec<Summary> = lines[start..]
        .iter()
        .filter_map(|l| Summary::from_jsonl_line(l))
        .collect();
    summaries.sort_by(|a, b| b.ts.cmp(&a.ts));
    summaries.truncate(n);
    summaries
}

pub fn write_summary(memory_dir: &Path, summary: &Summary) {
    let path = memory_dir.join("summaries.jsonl");
    let offset = jsonl_current_size(memory_dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{}", summary.to_jsonl_line());
    }
    // Append to index
    let idx_path = index_path(memory_dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&idx_path)
    {
        let _ = writeln!(f, "{}\t{}", summary.ts, offset);
    }
}

// ── Offset index helpers ──────────────────────────────────────────────────

fn index_path(memory_dir: &Path) -> std::path::PathBuf {
    memory_dir.join("summaries.index")
}

/// Reads (timestamp, byte_offset) pairs from summaries.index.
/// Returns empty vec if missing or malformed.
fn read_index(memory_dir: &Path) -> Vec<(u64, u64)> {
    let content = match std::fs::read_to_string(index_path(memory_dir)) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let mut parts = l.splitn(2, '\t');
            let ts: u64 = parts.next()?.trim().parse().ok()?;
            let off: u64 = parts.next()?.trim().parse().ok()?;
            Some((ts, off))
        })
        .collect()
}

/// Rebuilds summaries.index by scanning summaries.jsonl once (O(n) one-time cost).
/// Called automatically when index is absent or stale.
pub fn rebuild_index(memory_dir: &Path) {
    use std::io::{BufRead, Write as _};
    let jsonl_path = memory_dir.join("summaries.jsonl");
    let file = match std::fs::File::open(&jsonl_path) {
        Ok(f) => f,
        Err(_) => return,
    };
    let mut reader = std::io::BufReader::new(file);
    let mut entries: Vec<(u64, u64)> = Vec::new();
    let mut offset: u64 = 0;
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(n_read) => {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    if let Some(ts) = crate::json_util::extract_u64(trimmed, "ts") {
                        entries.push((ts, offset));
                    }
                }
                offset += n_read as u64;
            }
            Err(_) => break,
        }
    }
    let tmp = index_path(memory_dir).with_extension("index.tmp");
    if let Ok(mut f) = std::fs::File::create(&tmp) {
        for (ts, off) in &entries {
            let _ = writeln!(f, "{}\t{}", ts, off);
        }
        let _ = std::fs::rename(&tmp, index_path(memory_dir));
    }
}

/// Returns byte offset of summaries.jsonl just before writing, for index tracking.
fn jsonl_current_size(memory_dir: &Path) -> u64 {
    std::fs::metadata(memory_dir.join("summaries.jsonl"))
        .map(|m| m.len())
        .unwrap_or(0)
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
    let mut files_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut errors: Vec<String> = Vec::new();
    let mut errors_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut git_evts: Vec<String> = Vec::new();
    let mut git_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
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
            if files_seen.insert(f.clone()) {
                files.push(f);
            }
        }
        for e in extract_str_array(line, "errors") {
            if errors_seen.insert(e.clone()) {
                errors.push(e);
            }
        }
        for g in extract_str_array(line, "git") {
            if git_seen.insert(g.clone()) {
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

    // Try index-assisted binary search first
    let index = read_index(memory_dir);
    let keep_from_line = if !index.is_empty() {
        // index is sorted by ts ascending (append order)
        // find first position where ts >= cutoff
        index.partition_point(|(ts, _)| *ts < cutoff)
    } else {
        0
    };

    let tmp = path.with_extension("tmp");
    if let Ok(mut f) = std::fs::File::create(&tmp) {
        let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        let use_index = !index.is_empty();
        for (i, line) in lines.iter().enumerate() {
            let keep = if use_index {
                i >= keep_from_line
            } else {
                // Fallback: filter by effective_ts for lines without index
                let eff = effective_ts(line);
                eff == 0 || eff >= cutoff
            };
            if keep {
                let _ = writeln!(f, "{}", line);
            }
        }
        let _ = std::fs::rename(&tmp, &path);
    }
    // Rebuild index after pruning
    rebuild_index(memory_dir);
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

    fn tmp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "squeez_mem_{}_{}",
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ))
    }

    #[test]
    fn test_read_last_n_tail_only() {
        let dir = tmp_dir("tail");
        std::fs::create_dir_all(&dir).unwrap();
        // Write 100 entries with distinct timestamps
        for i in 1..=100u64 {
            let mut entry = s(i, 0);
            entry.date = format!("2026-04-{:02}", (i % 28) + 1);
            write_summary(&dir, &entry);
        }
        let result = read_last_n(&dir, 3);
        assert_eq!(result.len(), 3);
        // Should be the 3 most recent (highest ts), sorted descending
        assert_eq!(result[0].ts, 100);
        assert_eq!(result[1].ts, 99);
        assert_eq!(result[2].ts, 98);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_last_n_empty_file() {
        let dir = tmp_dir("empty");
        std::fs::create_dir_all(&dir).unwrap();
        // Create an empty file
        std::fs::write(dir.join("summaries.jsonl"), "").unwrap();
        let result = read_last_n(&dir, 5);
        assert!(result.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_last_n_fewer_than_n() {
        let dir = tmp_dir("fewer");
        std::fs::create_dir_all(&dir).unwrap();
        write_summary(&dir, &s(100, 0));
        write_summary(&dir, &s(200, 0));
        let result = read_last_n(&dir, 5);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].ts, 200);
        assert_eq!(result[1].ts, 100);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_index_maintained_on_write() {
        let dir = tmp_dir("idx_write");
        std::fs::create_dir_all(&dir).unwrap();
        write_summary(&dir, &s(1000, 0));
        write_summary(&dir, &s(2000, 0));
        write_summary(&dir, &s(3000, 0));
        let index = read_index(&dir);
        assert_eq!(index.len(), 3);
        assert_eq!(index[0].0, 1000);
        assert_eq!(index[1].0, 2000);
        assert_eq!(index[2].0, 3000);
        // Offsets should be monotonically increasing
        assert!(index[1].1 > index[0].1);
        assert!(index[2].1 > index[1].1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_rebuild_index() {
        let dir = tmp_dir("rebuild");
        std::fs::create_dir_all(&dir).unwrap();
        // Write summaries directly without index
        let path = dir.join("summaries.jsonl");
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&path).unwrap();
            writeln!(f, "{}", s(100, 0).to_jsonl_line()).unwrap();
            writeln!(f, "{}", s(200, 0).to_jsonl_line()).unwrap();
            writeln!(f, "{}", s(300, 0).to_jsonl_line()).unwrap();
        }
        // No index file yet
        assert!(read_index(&dir).is_empty());
        // Rebuild
        rebuild_index(&dir);
        let index = read_index(&dir);
        assert_eq!(index.len(), 3);
        assert_eq!(index[0].0, 100);
        assert_eq!(index[1].0, 200);
        assert_eq!(index[2].0, 300);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_prune_uses_binary_search() {
        let dir = tmp_dir("prune_bs");
        std::fs::create_dir_all(&dir).unwrap();
        let now = crate::session::unix_now();
        // Write entries: 2 old (2 days ago), 2 recent (now)
        let old_ts = now.saturating_sub(2 * 86400);
        write_summary(&dir, &s(old_ts, 0));
        write_summary(&dir, &s(old_ts + 1, 0));
        write_summary(&dir, &s(now, 0));
        write_summary(&dir, &s(now + 1, 0));
        // Verify index has 4 entries
        assert_eq!(read_index(&dir).len(), 4);
        // Prune with 1-day retention
        prune_old(&dir, 1);
        // Should keep only the 2 recent entries
        let remaining = read_last_n(&dir, 10);
        assert_eq!(remaining.len(), 2);
        assert!(remaining[0].ts >= now);
        assert!(remaining[1].ts >= now);
        // Index should be rebuilt with 2 entries
        let index = read_index(&dir);
        assert_eq!(index.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
