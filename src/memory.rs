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
}

impl Summary {
    pub fn to_jsonl_line(&self) -> String {
        format!(
            "{{\"date\":\"{}\",\"duration_min\":{},\"tokens_saved\":{},\
\"files_touched\":{},\"files_committed\":{},\"test_summary\":\"{}\",\
\"errors_resolved\":{},\"git_events\":{},\"ts\":{}}}",
            escape_str(&self.date),
            self.duration_min,
            self.tokens_saved,
            str_array(&self.files_touched),
            str_array(&self.files_committed),
            escape_str(&self.test_summary),
            str_array(&self.errors_resolved),
            str_array(&self.git_events),
            self.ts,
        )
    }

    pub fn from_jsonl_line(line: &str) -> Option<Self> {
        Some(Self {
            date: extract_str(line, "date")?,
            duration_min: extract_u64(line, "duration_min").unwrap_or(0),
            tokens_saved: extract_u64(line, "tokens_saved").unwrap_or(0),
            files_touched: extract_str_array(line, "files_touched"),
            files_committed: extract_str_array(line, "files_committed"),
            test_summary: extract_str(line, "test_summary").unwrap_or_default(),
            errors_resolved: extract_str_array(line, "errors_resolved"),
            git_events: extract_str_array(line, "git_events"),
            ts: extract_u64(line, "ts").unwrap_or(0),
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
    let kept: Vec<&str> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter(|l| extract_u64(l, "ts").map(|ts| ts >= cutoff).unwrap_or(true))
        .collect();
    let _ = std::fs::write(&path, kept.join("\n") + "\n");
}
