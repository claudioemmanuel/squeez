use std::path::Path;

use crate::context::hash::{fnv1a_64, short_hex};
use crate::json_util;

// ── Bounds ─────────────────────────────────────────────────────────────────

const MAX_CALL_LOG: usize = 32;
const MAX_SEEN_FILES: usize = 256;
const MAX_SEEN_ERRORS: usize = 128;
const MAX_SEEN_GIT_REFS: usize = 64;

/// How many recent calls are eligible for redundancy lookup.
pub const RECENT_WINDOW: u64 = 8;

// ── Data structures ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CallEntry {
    pub call_n: u64,
    pub cmd_short: String, // first 40 chars of cmd
    pub output_hash: u64,
    pub output_len: usize,
    pub short_hash: String, // 8 hex chars
}

#[derive(Debug, Clone)]
pub struct FileFingerprint {
    pub path: String,
    pub size_class: u32, // bytes / 4096
    pub last_seen_call: u64,
}

#[derive(Debug, Default, Clone)]
pub struct SessionContext {
    pub session_file: String,
    pub call_counter: u64,
    pub seen_files: Vec<FileFingerprint>,
    pub seen_errors: Vec<u64>, // FNV of normalized error
    pub seen_git_refs: Vec<String>, // 7-char SHAs
    pub call_log: Vec<CallEntry>,
}

// ── Public API ─────────────────────────────────────────────────────────────

impl SessionContext {
    pub fn load(sessions_dir: &Path) -> Self {
        let path = sessions_dir.join("context.json");
        let content = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return Self::default(),
        };
        Self::from_json(&content)
    }

    pub fn save(&self, sessions_dir: &Path) {
        let _ = std::fs::create_dir_all(sessions_dir);
        let path = sessions_dir.join("context.json");
        let json = self.to_json();
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
            {
                let _ = f.write_all(json.as_bytes());
            }
        }
        #[cfg(not(unix))]
        {
            let _ = std::fs::write(path, json);
        }
    }

    pub fn next_call_n(&mut self) -> u64 {
        self.call_counter = self.call_counter.saturating_add(1);
        self.call_counter
    }

    pub fn record_call(
        &mut self,
        cmd: &str,
        output_hash: u64,
        output_len: usize,
        call_n: u64,
    ) {
        let short = short_hex(output_hash);
        let cmd_short: String = cmd.chars().take(40).collect();
        self.call_log.push(CallEntry {
            call_n,
            cmd_short,
            output_hash,
            output_len,
            short_hash: short,
        });
        if self.call_log.len() > MAX_CALL_LOG {
            let drop_n = self.call_log.len() - MAX_CALL_LOG;
            self.call_log.drain(0..drop_n);
        }
    }

    /// Lookup a recent call with matching hash AND output_len. Only considers
    /// the last RECENT_WINDOW entries.
    pub fn lookup_recent(&self, hash: u64, len: usize) -> Option<&CallEntry> {
        let start = self.call_log.len().saturating_sub(RECENT_WINDOW as usize);
        self.call_log[start..]
            .iter()
            .find(|e| e.output_hash == hash && e.output_len == len)
    }

    pub fn note_files(&mut self, files: &[String]) {
        let call_n = self.call_counter;
        for f in files {
            if let Some(existing) = self.seen_files.iter_mut().find(|fp| fp.path == *f) {
                existing.last_seen_call = call_n;
            } else {
                self.seen_files.push(FileFingerprint {
                    path: f.clone(),
                    size_class: 0,
                    last_seen_call: call_n,
                });
            }
        }
        if self.seen_files.len() > MAX_SEEN_FILES {
            let drop_n = self.seen_files.len() - MAX_SEEN_FILES;
            self.seen_files.drain(0..drop_n);
        }
    }

    pub fn note_errors(&mut self, errors: &[String]) {
        for e in errors {
            let fp = fnv1a_64(normalize_error(e).as_bytes());
            if !self.seen_errors.contains(&fp) {
                self.seen_errors.push(fp);
            }
        }
        if self.seen_errors.len() > MAX_SEEN_ERRORS {
            let drop_n = self.seen_errors.len() - MAX_SEEN_ERRORS;
            self.seen_errors.drain(0..drop_n);
        }
    }

    pub fn note_git(&mut self, refs: &[String]) {
        for r in refs {
            // first 7 chars of any line, if hex
            let sha: String = r
                .trim()
                .chars()
                .take(7)
                .filter(|c| c.is_ascii_hexdigit())
                .collect();
            if sha.len() == 7 && !self.seen_git_refs.contains(&sha) {
                self.seen_git_refs.push(sha);
            }
        }
        if self.seen_git_refs.len() > MAX_SEEN_GIT_REFS {
            let drop_n = self.seen_git_refs.len() - MAX_SEEN_GIT_REFS;
            self.seen_git_refs.drain(0..drop_n);
        }
    }

    pub fn file_was_seen(&self, path: &str) -> Option<u64> {
        self.seen_files
            .iter()
            .find(|f| f.path == path)
            .map(|f| f.last_seen_call)
    }
}

// ── Cross-call hint ────────────────────────────────────────────────────────

/// If `cmd` is a raw read of a file already in context, return a hint line.
/// Recognised: cat, head, tail, less, more, bat.
pub fn raw_read_hint(ctx: &SessionContext, cmd: &str) -> Option<String> {
    let mut parts = cmd.trim().split_whitespace();
    let prog = parts.next()?;
    let prog = prog.rsplit('/').next().unwrap_or(prog);
    if !matches!(prog, "cat" | "head" | "tail" | "less" | "more" | "bat") {
        return None;
    }
    for arg in parts {
        if arg.starts_with('-') {
            continue;
        }
        if let Some(call_n) = ctx.file_was_seen(arg) {
            return Some(format!(
                "# squeez hint: {} already in context (Read tool, call #{}) — consider --no-squeez or skip",
                arg, call_n
            ));
        }
    }
    None
}

// ── Error normalization ────────────────────────────────────────────────────

/// Normalize an error string before hashing for fingerprinting:
/// lowercase → trim → digit runs → N → /paths → PATH → hex≥6 → HEX → trunc 200.
pub fn normalize_error(s: &str) -> String {
    let lower = s.trim().to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let chars: Vec<char> = lower.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // Path: a / followed by non-space chars
        if c == '/' {
            let mut j = i + 1;
            while j < chars.len() && !chars[j].is_whitespace() && chars[j] != '"' {
                j += 1;
            }
            if j > i + 1 {
                out.push_str("PATH");
                i = j;
                continue;
            }
        }
        // Digit run
        if c.is_ascii_digit() {
            let mut j = i;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            out.push('N');
            i = j;
            continue;
        }
        // Hex run ≥6 chars (after digit check so pure-digit doesn't match)
        if c.is_ascii_hexdigit() {
            let mut j = i;
            while j < chars.len() && chars[j].is_ascii_hexdigit() {
                j += 1;
            }
            if j - i >= 6 {
                out.push_str("HEX");
                i = j;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out.chars().take(200).collect()
}

// ── (de)serialization (hand-rolled, parallel arrays) ───────────────────────

impl SessionContext {
    pub fn to_json(&self) -> String {
        // Parallel arrays for call_log
        let cl_n: Vec<u64> = self.call_log.iter().map(|c| c.call_n).collect();
        let cl_cmd: Vec<String> = self.call_log.iter().map(|c| c.cmd_short.clone()).collect();
        let cl_hash: Vec<u64> = self.call_log.iter().map(|c| c.output_hash).collect();
        let cl_len: Vec<usize> = self.call_log.iter().map(|c| c.output_len).collect();
        let cl_short: Vec<String> = self.call_log.iter().map(|c| c.short_hash.clone()).collect();

        let sf_path: Vec<String> = self.seen_files.iter().map(|f| f.path.clone()).collect();
        let sf_size: Vec<u64> =
            self.seen_files.iter().map(|f| f.size_class as u64).collect();
        let sf_last: Vec<u64> = self.seen_files.iter().map(|f| f.last_seen_call).collect();

        format!(
            "{{\"session_file\":\"{}\",\"call_counter\":{},\
\"call_log_n\":{},\"call_log_cmd\":{},\"call_log_hash\":{},\"call_log_len\":{},\"call_log_short\":{},\
\"seen_files_path\":{},\"seen_files_size\":{},\"seen_files_last\":{},\
\"seen_errors\":{},\"seen_git_refs\":{}}}",
            json_util::escape_str(&self.session_file),
            self.call_counter,
            json_util::u64_array(&cl_n),
            json_util::str_array(&cl_cmd),
            json_util::u64_array(&cl_hash),
            json_util::usize_array(&cl_len),
            json_util::str_array(&cl_short),
            json_util::str_array(&sf_path),
            json_util::u64_array(&sf_size),
            json_util::u64_array(&sf_last),
            json_util::u64_array(&self.seen_errors),
            json_util::str_array(&self.seen_git_refs),
        )
    }

    pub fn from_json(s: &str) -> Self {
        let mut c = Self::default();
        c.session_file = json_util::extract_str(s, "session_file").unwrap_or_default();
        c.call_counter = json_util::extract_u64(s, "call_counter").unwrap_or(0);

        let cl_n = json_util::extract_u64_array(s, "call_log_n");
        let cl_cmd = json_util::extract_str_array(s, "call_log_cmd");
        let cl_hash = json_util::extract_u64_array(s, "call_log_hash");
        let cl_len = json_util::extract_u64_array(s, "call_log_len");
        let cl_short = json_util::extract_str_array(s, "call_log_short");
        let n = cl_n
            .len()
            .min(cl_cmd.len())
            .min(cl_hash.len())
            .min(cl_len.len())
            .min(cl_short.len());
        for i in 0..n {
            c.call_log.push(CallEntry {
                call_n: cl_n[i],
                cmd_short: cl_cmd[i].clone(),
                output_hash: cl_hash[i],
                output_len: cl_len[i] as usize,
                short_hash: cl_short[i].clone(),
            });
        }

        let sf_path = json_util::extract_str_array(s, "seen_files_path");
        let sf_size = json_util::extract_u64_array(s, "seen_files_size");
        let sf_last = json_util::extract_u64_array(s, "seen_files_last");
        let m = sf_path.len().min(sf_size.len()).min(sf_last.len());
        for i in 0..m {
            c.seen_files.push(FileFingerprint {
                path: sf_path[i].clone(),
                size_class: sf_size[i] as u32,
                last_seen_call: sf_last[i],
            });
        }

        c.seen_errors = json_util::extract_u64_array(s, "seen_errors");
        c.seen_git_refs = json_util::extract_str_array(s, "seen_git_refs");
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_replaces_digits_paths_hex() {
        let n = normalize_error("Error: file /tmp/foo/bar.txt line 42 abc123def");
        assert!(n.contains("PATH"), "got: {}", n);
        assert!(n.contains('N'), "got: {}", n);
        assert!(!n.contains("/tmp/foo"));
    }

    #[test]
    fn record_call_drops_oldest_at_33rd() {
        let mut c = SessionContext::default();
        for i in 0..40 {
            let n = c.next_call_n();
            c.record_call(&format!("cmd{}", i), i, i as usize, n);
        }
        assert_eq!(c.call_log.len(), MAX_CALL_LOG);
        // Oldest entries dropped
        assert_eq!(c.call_log[0].call_n, 9); // calls 1..=8 dropped
    }

    #[test]
    fn lookup_recent_only_within_window() {
        let mut c = SessionContext::default();
        for i in 1..=20u64 {
            c.next_call_n();
            c.record_call(&format!("c{}", i), i * 10, i as usize, i);
        }
        // Last call hash present
        assert!(c.lookup_recent(200, 20).is_some());
        // Older than window — should be None
        assert!(c.lookup_recent(50, 5).is_none());
    }

    #[test]
    fn note_files_dedup_and_caps() {
        let mut c = SessionContext::default();
        c.next_call_n();
        for i in 0..300 {
            c.note_files(&[format!("/path/{}.rs", i)]);
        }
        assert!(c.seen_files.len() <= MAX_SEEN_FILES);
    }

    #[test]
    fn file_was_seen_returns_call_n() {
        let mut c = SessionContext::default();
        c.next_call_n();
        c.note_files(&["/foo.rs".to_string()]);
        assert_eq!(c.file_was_seen("/foo.rs"), Some(1));
        assert_eq!(c.file_was_seen("/bar.rs"), None);
    }

    #[test]
    fn raw_read_hint_detects_seen_file() {
        let mut c = SessionContext::default();
        c.next_call_n();
        c.note_files(&["/foo.rs".to_string()]);
        let hint = raw_read_hint(&c, "cat /foo.rs");
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("/foo.rs"));
    }

    #[test]
    fn raw_read_hint_ignores_unknown_program() {
        let c = SessionContext::default();
        assert!(raw_read_hint(&c, "git status").is_none());
    }

    #[test]
    fn json_round_trip() {
        let mut c = SessionContext::default();
        c.session_file = "2026-04-07-10.jsonl".to_string();
        c.next_call_n();
        c.record_call("git status", 0xdead_beef, 100, 1);
        c.note_files(&["/a.rs".to_string(), "/b.rs".to_string()]);
        c.note_errors(&["error: cannot find function 'foo'".to_string()]);

        let json = c.to_json();
        let r = SessionContext::from_json(&json);
        assert_eq!(r.session_file, c.session_file);
        assert_eq!(r.call_counter, c.call_counter);
        assert_eq!(r.call_log.len(), 1);
        assert_eq!(r.call_log[0].output_hash, 0xdead_beef);
        assert_eq!(r.call_log[0].output_len, 100);
        assert_eq!(r.seen_files.len(), 2);
        assert_eq!(r.seen_errors.len(), 1);
    }

    #[test]
    fn save_load_round_trip_via_disk() {
        let dir = std::env::temp_dir().join(format!(
            "squeez_ctx_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let mut c = SessionContext::default();
        c.session_file = "test.jsonl".to_string();
        c.next_call_n();
        c.record_call("ls", 42, 10, 1);
        c.save(&dir);

        let loaded = SessionContext::load(&dir);
        assert_eq!(loaded.session_file, "test.jsonl");
        assert_eq!(loaded.call_counter, 1);
        assert_eq!(loaded.call_log.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
