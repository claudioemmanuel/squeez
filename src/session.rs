use std::path::{Path, PathBuf};
use std::io::Write;

// ── Directory helpers ──────────────────────────────────────────────────────

/// Returns the squeez state directory. Overridable via SQUEEZ_DIR env var (for tests).
pub fn squeez_dir() -> PathBuf {
    if let Ok(d) = std::env::var("SQUEEZ_DIR") {
        return PathBuf::from(d);
    }
    PathBuf::from(format!("{}/.claude/squeez",
        std::env::var("HOME").unwrap_or_default()))
}

pub fn sessions_dir() -> PathBuf { squeez_dir().join("sessions") }
pub fn memory_dir() -> PathBuf   { squeez_dir().join("memory") }

// ── Time helpers ───────────────────────────────────────────────────────────

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Returns "YYYY-MM-DD-HH.jsonl" for the current UTC time.
pub fn new_session_filename() -> String {
    format!("{}.jsonl", format_unix(unix_now()))
}

/// Returns "YYYY-MM-DD" for the given unix timestamp.
pub fn unix_to_date(secs: u64) -> String {
    let (y, m, d) = days_to_ymd(secs / 86400);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn format_unix(secs: u64) -> String {
    let hour = (secs % 86400) / 3600;
    let (y, m, d) = days_to_ymd(secs / 86400);
    format!("{:04}-{:02}-{:02}-{:02}", y, m, d, hour)
}

fn days_to_ymd(days: u64) -> (u32, u32, u32) {
    let mut rem = days as i64;
    let mut year = 1970i32;
    loop {
        let diy: i64 = if is_leap(year) { 366 } else { 365 };
        if rem < diy { break; }
        rem -= diy;
        year += 1;
    }
    let month_days: [i64; 12] = [
        31, if is_leap(year) { 29 } else { 28 }, 31, 30, 31, 30,
        31, 31, 30, 31, 30, 31,
    ];
    let mut month = 1u32;
    for &md in &month_days {
        if rem < md { break; }
        rem -= md;
        month += 1;
    }
    (year as u32, month, rem as u32 + 1)
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

// ── CurrentSession ─────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct CurrentSession {
    pub session_file: String,
    pub total_tokens: u64,
    pub compact_warned: bool,
    pub start_ts: u64,
}

impl CurrentSession {
    pub fn load(sessions_dir: &Path) -> Option<Self> {
        let s = std::fs::read_to_string(sessions_dir.join("current.json")).ok()?;
        Some(Self {
            session_file:   crate::json_util::extract_str(&s, "session_file").unwrap_or_default(),
            total_tokens:   crate::json_util::extract_u64(&s, "total_tokens").unwrap_or(0),
            compact_warned: crate::json_util::extract_bool(&s, "compact_warned").unwrap_or(false),
            start_ts:       crate::json_util::extract_u64(&s, "start_ts").unwrap_or(0),
        })
    }

    pub fn save(&self, sessions_dir: &Path) {
        let json = format!(
            "{{\"session_file\":\"{}\",\"total_tokens\":{},\"compact_warned\":{},\"start_ts\":{}}}",
            self.session_file.replace('"', ""),
            self.total_tokens,
            self.compact_warned,
            self.start_ts,
        );
        let _ = std::fs::write(sessions_dir.join("current.json"), json);
    }
}

// ── Event log ──────────────────────────────────────────────────────────────

/// Appends one JSONL line to the session log file (creates if missing).
pub fn append_event(sessions_dir: &Path, session_file: &str, event_json: &str) {
    if session_file.is_empty() { return; }
    let path = sessions_dir.join(session_file);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true).append(true).open(&path)
    {
        let _ = writeln!(f, "{}", event_json);
    }
}
