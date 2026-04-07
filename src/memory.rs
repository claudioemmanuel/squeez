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
    /// Start of the validity window. Summaries can be invalidated at a known
    /// timestamp so that `prune_old` ages them from `valid_to` rather than `ts`.
    pub valid_from: u64,
    /// End of the validity window. `0` means "still valid" (open-ended).
    /// When non-zero, this summary's facts are considered superseded as of
    /// that timestamp — `prune_old` then ages it from `valid_to` rather than
    /// from `ts`, so an invalidated summary doesn't outlive its retention.
    pub valid_to: u64,
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
\"valid_from\":{},\"valid_to\":{}}}",
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
        )
    }

    pub fn from_jsonl_line(line: &str) -> Option<Self> {
        let ts = extract_u64(line, "ts").unwrap_or(0);
        // Both new fields are optional for backwards compat with summaries
        // written by squeez < 0.3 (no temporal validity columns).
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
        format!("Prior session ({}): {}{}", self.date, files, git)
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
