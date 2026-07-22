use std::io::Write;
use std::path::{Path, PathBuf};

// ── Directory helpers ──────────────────────────────────────────────────────

/// Returns the user's home directory.
/// Tries `HOME` first (Unix/Git Bash), then `USERPROFILE` (Windows native).
pub fn home_dir() -> String {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default()
}

/// Returns the squeez state directory. Overridable via SQUEEZ_DIR env var (for tests).
pub fn squeez_dir() -> PathBuf {
    if let Ok(d) = std::env::var("SQUEEZ_DIR") {
        return PathBuf::from(d);
    }
    PathBuf::from(format!("{}/.claude/squeez", home_dir()))
}

pub fn sessions_dir() -> PathBuf {
    squeez_dir().join("sessions")
}
pub fn memory_dir() -> PathBuf {
    squeez_dir().join("memory")
}

/// Atomically claim a one-shot advisory key. Returns `true` for the first
/// caller and `false` for every later one, for the life of the session.
///
/// `SessionContext.nudged_keys` cannot do this on its own: every hook runs in
/// a separate short-lived process that loads `context.json`, mutates it, and
/// writes it back whole. `squeez wrap` holds its loaded copy across the entire
/// subprocess run — seconds, sometimes minutes — so a `PostToolUse` process
/// that loads and saves in that window has its key silently overwritten by
/// wrap's stale copy. Observed live: a "fire once per session" advisory
/// emitted 11 times in one session while `nudged_keys` stayed empty on disk.
///
/// A file created with `create_new` is an atomic test-and-set at the
/// filesystem level, which is exactly the primitive the guard needs, with no
/// dependency and no lock file to leak. Failures other than "already exists"
/// fail open: an advisory shown twice is cheaper than one lost.
pub fn claim_nudge(sessions_dir: &std::path::Path, key: &str) -> bool {
    let safe: String = key
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let dir = sessions_dir.join("nudges");
    if std::fs::create_dir_all(&dir).is_err() {
        return true;
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dir.join(safe))
    {
        Ok(_) => true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(_) => true,
    }
}

/// Drop every claimed advisory key. Called when a new session starts, so
/// one-shot advisories fire again in the next session.
pub fn reset_nudge_claims(sessions_dir: &std::path::Path) {
    let _ = std::fs::remove_dir_all(sessions_dir.join("nudges"));
}

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
        if rem < diy {
            break;
        }
        rem -= diy;
        year += 1;
    }
    let month_days: [i64; 12] = [
        31,
        if is_leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u32;
    for &md in &month_days {
        if rem < md {
            break;
        }
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
    pub tokens_saved: u64,
    pub total_calls: u64,
    pub compact_warned: bool,
    pub state_warned: bool,
    pub start_ts: u64,
    /// Cumulative token cost of squeez-authored emissions (header, retrieve
    /// marker, warnings, nudges) — the overhead side of the honest net-win
    /// ledger. Independent of `tokens_saved` so `net_saved` can go negative
    /// when overhead outweighs compression (E1).
    pub overhead_tokens: u64,
}

impl CurrentSession {
    pub fn load(sessions_dir: &Path) -> Option<Self> {
        let path = sessions_dir.join("current.json");
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size > crate::memory::MAX_FILE_BYTES {
            return None;
        }
        let s = std::fs::read_to_string(&path).ok()?;
        let map = crate::json_util::extract_all(&s);
        Some(Self {
            session_file: crate::json_util::map_str(&map, "session_file").unwrap_or_default(),
            total_tokens: crate::json_util::map_u64(&map, "total_tokens").unwrap_or(0),
            tokens_saved: crate::json_util::map_u64(&map, "tokens_saved").unwrap_or(0),
            total_calls: crate::json_util::map_u64(&map, "total_calls").unwrap_or(0),
            compact_warned: crate::json_util::map_bool(&map, "compact_warned").unwrap_or(false),
            state_warned: crate::json_util::map_bool(&map, "state_warned").unwrap_or(false),
            start_ts: crate::json_util::map_u64(&map, "start_ts").unwrap_or(0),
            overhead_tokens: crate::json_util::map_u64(&map, "overhead_tokens").unwrap_or(0),
        })
    }

    pub fn save(&self, sessions_dir: &Path) {
        let json = format!(
            "{{\"session_file\":\"{}\",\"total_tokens\":{},\"tokens_saved\":{},\"total_calls\":{},\"compact_warned\":{},\"state_warned\":{},\"start_ts\":{},\"overhead_tokens\":{}}}",
            crate::json_util::escape_str(&self.session_file),
            self.total_tokens,
            self.tokens_saved,
            self.total_calls,
            self.compact_warned,
            self.state_warned,
            self.start_ts,
            self.overhead_tokens,
        );
        let path = sessions_dir.join("current.json");
        let tmp = path.with_extension("json.tmp");
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)
            {
                use std::io::Write;
                let _ = f.write_all(json.as_bytes());
            }
        }
        #[cfg(not(unix))]
        {
            let _ = std::fs::write(&tmp, &json);
        }
        let _ = std::fs::rename(&tmp, &path);
    }
}

// ── Event log ──────────────────────────────────────────────────────────────

const MAX_SESSION_LOG_BYTES: u64 = 20 * 1024 * 1024;

/// Appends one JSONL line to the session log file (creates if missing).
/// Silently skips if the file exceeds MAX_SESSION_LOG_BYTES.
pub fn append_event(sessions_dir: &Path, session_file: &str, event_json: &str) {
    if session_file.is_empty() || session_file.contains('/') || session_file.contains("..") {
        return;
    }
    let path = sessions_dir.join(session_file);
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > MAX_SESSION_LOG_BYTES {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&path)
        {
            let _ = writeln!(f, "{}", event_json);
        }
    }
    #[cfg(not(unix))]
    {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(f, "{}", event_json);
        }
    }
}
