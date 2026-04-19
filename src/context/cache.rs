use std::path::Path;

use crate::config::Config;
use crate::context::hash::{fnv1a_64, jaccard, short_hex};
use crate::json_util;

// ── Bounds ─────────────────────────────────────────────────────────────────

const MAX_SEEN_FILES: usize = 256;
const MAX_SEEN_ERRORS: usize = 128;
const MAX_SEEN_GIT_REFS: usize = 64;

/// Default max entries in the rolling call log. Overridable via config.
pub const DEFAULT_MAX_CALL_LOG: usize = 32;
/// Default recent-window size for redundancy lookup. Overridable via config.
pub const DEFAULT_RECENT_WINDOW: usize = 16;
/// Default minimum Jaccard similarity threshold. Overridable via config.
pub const DEFAULT_SIMILARITY_THRESHOLD: f32 = 0.85;

// Keep these pub aliases for any code that still imports them by old name.
#[allow(dead_code)]
pub const RECENT_WINDOW: u64 = DEFAULT_RECENT_WINDOW as u64;
#[allow(dead_code)]
pub const SIMILARITY_THRESHOLD: f32 = DEFAULT_SIMILARITY_THRESHOLD;

/// Allowed length ratio (in either direction) for similarity matching.
pub const LENGTH_RATIO_GUARD: f32 = 0.80;

// ── FileAccess ──────────────────────────────────────────────────────────────

/// How a file was accessed during a call. Used to enrich `squeez_seen_files`.
#[derive(Debug, Clone, PartialEq)]
pub enum FileAccess {
    Read,
    Write,
    Created,
    Deleted,
}

impl FileAccess {
    pub fn as_char(&self) -> char {
        match self {
            FileAccess::Read => 'R',
            FileAccess::Write => 'W',
            FileAccess::Created => 'C',
            FileAccess::Deleted => 'D',
        }
    }

    pub fn from_char(c: char) -> Self {
        match c {
            'W' => FileAccess::Write,
            'C' => FileAccess::Created,
            'D' => FileAccess::Deleted,
            _ => FileAccess::Read,
        }
    }

    pub fn as_label(&self) -> &'static str {
        match self {
            FileAccess::Read => "read",
            FileAccess::Write => "write",
            FileAccess::Created => "created",
            FileAccess::Deleted => "deleted",
        }
    }
}

// ── Data structures ────────────────────────────────────────────────────────

/// Cap on tracked agent spawn entries (rolling window).
pub const MAX_AGENT_SPAWN_LOG: usize = 16;
/// Cap on burn rate sliding window entries.
pub const MAX_BURN_WINDOW: usize = 16;

#[derive(Debug, Clone)]
pub struct AgentSpawnEntry {
    pub call_n: u64,
    pub tool_name: String,
    pub estimated_tokens: u64,
    pub ts: u64,
}

#[derive(Debug, Clone)]
pub struct BurnEntry {
    pub call_n: u64,
    pub tokens: u64,
    pub ts: u64,
}

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
    /// How the file was last accessed (phase 4).
    pub access: FileAccess,
}

#[derive(Debug, Clone)]
pub struct SessionContext {
    pub session_file: String,
    pub call_counter: u64,
    pub seen_files: Vec<FileFingerprint>,
    pub seen_errors: Vec<u64>, // FNV of normalized error
    /// First-128-char snippets parallel to `seen_errors` (phase 2).
    /// Each entry is `(fingerprint, snippet_text)` in insertion order.
    pub error_snippets: Vec<(u64, String)>,
    pub seen_git_refs: Vec<String>, // 7-char SHAs
    pub call_log: Vec<CallEntry>,
    /// Bottom-k MinHash sketch parallel to `call_log`. Each entry is the
    /// sorted-deduplicated trigram-shingle hash set for that call's output.
    /// Used by `lookup_similar` for fuzzy redundancy matching that survives
    /// whitespace/timestamp/single-line-edit perturbations.
    ///
    /// May be shorter than `call_log` after loading older context.json files
    /// that pre-date this field; callers must check length parity defensively.
    pub call_log_shingles: Vec<Vec<u64>>,
    /// Cumulative token counts by tool category (Bash, Read, Grep, Other)
    pub tokens_bash: u64,
    pub tokens_read: u64,
    pub tokens_grep: u64,
    pub tokens_other: u64,
    /// How many times a file was accessed that was already in seen_files (re-read metric).
    pub reread_count: u32,
    // ── Compression statistics (phase 6) ───────────────────────────────
    pub exact_dedup_hits: u32,
    pub fuzzy_dedup_hits: u32,
    pub summarize_triggers: u32,
    pub intensity_ultra_calls: u32,
    // ── Token economy (phase 7) ──────────────────────────────────────
    pub agent_spawns: u32,
    pub agent_estimated_tokens: u64,
    pub agent_spawn_log: Vec<AgentSpawnEntry>,
    pub burn_window: Vec<BurnEntry>,
    // ── Tunables (phase 5) — set from Config at session start, not persisted ─
    pub max_call_log: usize,
    pub recent_window: usize,
    pub similarity_threshold: f32,
}

impl Default for SessionContext {
    fn default() -> Self {
        Self {
            session_file: String::new(),
            call_counter: 0,
            seen_files: Vec::new(),
            seen_errors: Vec::new(),
            error_snippets: Vec::new(),
            seen_git_refs: Vec::new(),
            call_log: Vec::new(),
            call_log_shingles: Vec::new(),
            tokens_bash: 0,
            tokens_read: 0,
            tokens_grep: 0,
            tokens_other: 0,
            reread_count: 0,
            exact_dedup_hits: 0,
            fuzzy_dedup_hits: 0,
            summarize_triggers: 0,
            intensity_ultra_calls: 0,
            agent_spawns: 0,
            agent_estimated_tokens: 0,
            agent_spawn_log: Vec::new(),
            burn_window: Vec::new(),
            max_call_log: DEFAULT_MAX_CALL_LOG,
            recent_window: DEFAULT_RECENT_WINDOW,
            similarity_threshold: DEFAULT_SIMILARITY_THRESHOLD,
        }
    }
}

/// Result of `SessionContext::lookup_similar` — the matched call entry plus
/// the Jaccard similarity score (always ≥ `SIMILARITY_THRESHOLD`).
#[derive(Debug, Clone)]
pub struct SimilarMatch {
    pub call_n: u64,
    pub short_hash: String,
    pub similarity: f32,
}

// ── Public API ─────────────────────────────────────────────────────────────

impl SessionContext {
    pub fn load(sessions_dir: &Path) -> Self {
        let path = sessions_dir.join("context.json");
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size > crate::memory::MAX_FILE_BYTES {
            return Self::default();
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return Self::default(),
        };
        Self::from_json(&content)
    }

    /// Copy tunable values from Config into this context so all methods use
    /// the user's configured values rather than the compiled-in defaults.
    /// Called in `context::pre_pass` after loading or constructing the context.
    pub fn init_tunables_from_config(&mut self, cfg: &Config) {
        self.max_call_log = cfg.max_call_log.max(1);
        self.recent_window = cfg.recent_window as usize;
        self.similarity_threshold = cfg.similarity_threshold.clamp(0.0, 1.0);
    }

    pub fn save(&self, sessions_dir: &Path) {
        let _ = std::fs::create_dir_all(sessions_dir);
        let path = sessions_dir.join("context.json");
        let tmp = path.with_extension("json.tmp");
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
                .open(&tmp)
            {
                let _ = f.write_all(json.as_bytes());
            }
        }
        #[cfg(not(unix))]
        {
            let _ = std::fs::write(&tmp, &json);
        }
        let _ = std::fs::rename(&tmp, &path);
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
        self.record_call_with_shingles(cmd, output_hash, output_len, call_n, Vec::new());
    }

    /// Like `record_call`, but additionally stores a MinHash shingle sketch
    /// of the output so that `lookup_similar` can find near-matches later.
    pub fn record_call_with_shingles(
        &mut self,
        cmd: &str,
        output_hash: u64,
        output_len: usize,
        call_n: u64,
        shingles: Vec<u64>,
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
        // Keep shingles parallel to call_log (pad with empty if missing).
        while self.call_log_shingles.len() < self.call_log.len() - 1 {
            self.call_log_shingles.push(Vec::new());
        }
        self.call_log_shingles.push(shingles);
        if self.call_log.len() > self.max_call_log {
            let drop_n = self.call_log.len() - self.max_call_log;
            self.call_log.drain(0..drop_n);
            // Drop the same prefix from shingles to keep parity.
            let drop_s = self.call_log_shingles.len().min(drop_n);
            self.call_log_shingles.drain(0..drop_s);
        }
    }

    /// Lookup a recent call with matching hash AND output_len. Only considers
    /// the last `self.recent_window` entries.
    pub fn lookup_recent(&self, hash: u64, len: usize) -> Option<&CallEntry> {
        let start = self.call_log.len().saturating_sub(self.recent_window);
        self.call_log[start..]
            .iter()
            .find(|e| e.output_hash == hash && e.output_len == len)
    }

    /// Lookup the highest-similarity recent call whose Jaccard distance to
    /// `query_shingles` is at least `self.similarity_threshold` AND whose length
    /// ratio with `query_len` is within `LENGTH_RATIO_GUARD`. Considers only
    /// the last `self.recent_window` entries.
    ///
    /// Returns `None` when:
    /// - the query has no shingles (text too short for trigrams)
    /// - no candidate clears the threshold
    /// - shingles have not been recorded yet for the matching call (legacy load)
    pub fn lookup_similar(
        &self,
        query_shingles: &[u64],
        query_len: usize,
    ) -> Option<SimilarMatch> {
        if query_shingles.is_empty() {
            return None;
        }
        let log_len = self.call_log.len();
        let start = log_len.saturating_sub(self.recent_window);
        // Walk only the part of call_log that has parallel shingles.
        let s_len = self.call_log_shingles.len();
        // Calls without recorded shingles (older entries) are skipped silently.
        let mut best: Option<SimilarMatch> = None;
        for i in start..log_len {
            if i >= s_len {
                break;
            }
            let candidate_shingles = &self.call_log_shingles[i];
            if candidate_shingles.is_empty() {
                continue;
            }
            let entry = &self.call_log[i];
            // Length-ratio guard (symmetric): min/max ≥ LENGTH_RATIO_GUARD.
            let qlen = query_len.max(1) as f32;
            let elen = entry.output_len.max(1) as f32;
            let ratio = qlen.min(elen) / qlen.max(elen);
            if ratio < LENGTH_RATIO_GUARD {
                continue;
            }
            let sim = jaccard(query_shingles, candidate_shingles);
            if sim < self.similarity_threshold {
                continue;
            }
            let take = match &best {
                Some(b) => sim > b.similarity,
                None => true,
            };
            if take {
                best = Some(SimilarMatch {
                    call_n: entry.call_n,
                    short_hash: entry.short_hash.clone(),
                    similarity: sim,
                });
            }
        }
        best
    }

    /// Record a file access with an explicit access type (phase 4).
    pub fn note_file(&mut self, path: &str, access: FileAccess) {
        let call_n = self.call_counter;
        if let Some(existing) = self.seen_files.iter_mut().find(|fp| fp.path == path) {
            existing.last_seen_call = call_n;
            existing.access = access;
            self.reread_count = self.reread_count.saturating_add(1);
        } else {
            self.seen_files.push(FileFingerprint {
                path: path.to_string(),
                size_class: 0,
                last_seen_call: call_n,
                access,
            });
        }
        if self.seen_files.len() > MAX_SEEN_FILES {
            let drop_n = self.seen_files.len() - MAX_SEEN_FILES;
            self.seen_files.drain(0..drop_n);
        }
    }

    /// Record multiple files as Read access (backward-compatible wrapper).
    pub fn note_files(&mut self, files: &[String]) {
        for f in files {
            self.note_file(f, FileAccess::Read);
        }
    }

    pub fn note_errors(&mut self, errors: &[String]) {
        for e in errors {
            let fp = fnv1a_64(normalize_error(e).as_bytes());
            if !self.seen_errors.contains(&fp) {
                self.seen_errors.push(fp);
                // Phase 2: store first-128-chars snippet alongside fingerprint.
                // Sanitize [ and ] so the hand-rolled str_array/extract_str_array
                // parser (which uses ']' as array terminator) doesn't truncate.
                let snippet: String = e
                    .chars()
                    .take(128)
                    .map(|c| if c == '[' { '(' } else if c == ']' { ')' } else { c })
                    .collect();
                self.error_snippets.push((fp, snippet));
            }
        }
        if self.seen_errors.len() > MAX_SEEN_ERRORS {
            let drop_n = self.seen_errors.len() - MAX_SEEN_ERRORS;
            self.seen_errors.drain(0..drop_n);
            // Keep error_snippets cap in sync.
            if self.error_snippets.len() > MAX_SEEN_ERRORS {
                let drop_s = self.error_snippets.len() - MAX_SEEN_ERRORS;
                self.error_snippets.drain(0..drop_s);
            }
        }
    }

    // ── Phase 6 stat helpers ─────────────────────────────────────────────

    /// Record an exact-hash redundancy hit (called from wrap.rs after check()).
    pub fn note_redundancy_hit_exact(&mut self) {
        self.exact_dedup_hits = self.exact_dedup_hits.saturating_add(1);
    }

    /// Record a fuzzy-similarity redundancy hit (called from wrap.rs after check()).
    pub fn note_redundancy_hit_fuzzy(&mut self) {
        self.fuzzy_dedup_hits = self.fuzzy_dedup_hits.saturating_add(1);
    }

    /// Record that the summarizer was triggered for this call.
    pub fn note_summarize_trigger(&mut self) {
        self.summarize_triggers = self.summarize_triggers.saturating_add(1);
    }

    /// Record that Ultra intensity was active for this call.
    pub fn note_intensity_ultra(&mut self) {
        self.intensity_ultra_calls = self.intensity_ultra_calls.saturating_add(1);
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

    // ── Token economy helpers (phase 7) ────────────────────────────────

    /// Record a sub-agent spawn (Agent or Task tool call).
    pub fn note_agent_spawn(&mut self, tool_name: &str, estimated_tokens: u64) {
        self.agent_spawns = self.agent_spawns.saturating_add(1);
        self.agent_estimated_tokens = self.agent_estimated_tokens.saturating_add(estimated_tokens);
        self.agent_spawn_log.push(AgentSpawnEntry {
            call_n: self.call_counter,
            tool_name: tool_name.to_string(),
            estimated_tokens,
            ts: crate::session::unix_now(),
        });
        if self.agent_spawn_log.len() > MAX_AGENT_SPAWN_LOG {
            let drop_n = self.agent_spawn_log.len() - MAX_AGENT_SPAWN_LOG;
            self.agent_spawn_log.drain(0..drop_n);
        }
    }

    /// Record token consumption for burn rate prediction.
    pub fn note_burn(&mut self, tokens: u64) {
        self.burn_window.push(BurnEntry {
            call_n: self.call_counter,
            tokens,
            ts: crate::session::unix_now(),
        });
        if self.burn_window.len() > MAX_BURN_WINDOW {
            let drop_n = self.burn_window.len() - MAX_BURN_WINDOW;
            self.burn_window.drain(0..drop_n);
        }
    }

    /// Record token usage by tool category.
    pub fn note_tool_tokens(&mut self, tool: &str, tokens: u64) {
        match tool.to_lowercase().as_str() {
            "bash" => self.tokens_bash = self.tokens_bash.saturating_add(tokens),
            "read" => self.tokens_read = self.tokens_read.saturating_add(tokens),
            "grep" => self.tokens_grep = self.tokens_grep.saturating_add(tokens),
            _ => self.tokens_other = self.tokens_other.saturating_add(tokens),
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

        // Encode each shingle set as a `;`-joined string and wrap in str_array.
        // We use `;` rather than `,` because json_util::extract_str_array splits
        // its outer items on `,`, so commas inside string values would break
        // round-trip. Padding ensures parallelism with call_log even if some
        // entries pre-date the shingle field.
        let mut cl_sh_strs: Vec<String> = Vec::with_capacity(self.call_log.len());
        for i in 0..self.call_log.len() {
            let s = self
                .call_log_shingles
                .get(i)
                .map(|v| {
                    v.iter()
                        .map(|n| n.to_string())
                        .collect::<Vec<_>>()
                        .join(";")
                })
                .unwrap_or_default();
            cl_sh_strs.push(s);
        }

        let sf_path: Vec<String> = self.seen_files.iter().map(|f| f.path.clone()).collect();
        let sf_size: Vec<u64> =
            self.seen_files.iter().map(|f| f.size_class as u64).collect();
        let sf_last: Vec<u64> = self.seen_files.iter().map(|f| f.last_seen_call).collect();
        // Phase 4: file access types as single-char strings.
        let sf_access: Vec<String> = self
            .seen_files
            .iter()
            .map(|f| f.access.as_char().to_string())
            .collect();

        // Phase 2: error snippets as parallel arrays.
        let es_fp: Vec<u64> = self.error_snippets.iter().map(|(fp, _)| *fp).collect();
        let es_text: Vec<String> = self
            .error_snippets
            .iter()
            .map(|(_, t)| t.clone())
            .collect();

        // Phase 7: agent spawn log as parallel arrays.
        let as_call_n: Vec<u64> = self.agent_spawn_log.iter().map(|e| e.call_n).collect();
        let as_tool: Vec<String> = self.agent_spawn_log.iter().map(|e| e.tool_name.clone()).collect();
        let as_tokens: Vec<u64> = self.agent_spawn_log.iter().map(|e| e.estimated_tokens).collect();
        let as_ts: Vec<u64> = self.agent_spawn_log.iter().map(|e| e.ts).collect();

        // Phase 7: burn window as parallel arrays.
        let bw_call_n: Vec<u64> = self.burn_window.iter().map(|e| e.call_n).collect();
        let bw_tokens: Vec<u64> = self.burn_window.iter().map(|e| e.tokens).collect();
        let bw_ts: Vec<u64> = self.burn_window.iter().map(|e| e.ts).collect();

        format!(
            "{{\"session_file\":\"{}\",\"call_counter\":{},\
\"call_log_n\":{},\"call_log_cmd\":{},\"call_log_hash\":{},\"call_log_len\":{},\"call_log_short\":{},\
\"call_log_shingles\":{},\
\"seen_files_path\":{},\"seen_files_size\":{},\"seen_files_last\":{},\"seen_files_access\":{},\
\"seen_errors\":{},\"error_snippet_fp\":{},\"error_snippet_text\":{},\
\"seen_git_refs\":{},\
\"tokens_bash\":{},\"tokens_read\":{},\"tokens_grep\":{},\"tokens_other\":{},\"reread_count\":{},\
\"exact_dedup_hits\":{},\"fuzzy_dedup_hits\":{},\"summarize_triggers\":{},\"intensity_ultra_calls\":{},\
\"agent_spawns\":{},\"agent_estimated_tokens\":{},\
\"agent_spawn_log_call_n\":{},\"agent_spawn_log_tool\":{},\"agent_spawn_log_tokens\":{},\"agent_spawn_log_ts\":{},\
\"burn_window_call_n\":{},\"burn_window_tokens\":{},\"burn_window_ts\":{}}}",
            json_util::escape_str(&self.session_file),
            self.call_counter,
            json_util::u64_array(&cl_n),
            json_util::str_array(&cl_cmd),
            json_util::u64_array(&cl_hash),
            json_util::usize_array(&cl_len),
            json_util::str_array(&cl_short),
            json_util::str_array(&cl_sh_strs),
            json_util::str_array(&sf_path),
            json_util::u64_array(&sf_size),
            json_util::u64_array(&sf_last),
            json_util::str_array(&sf_access),
            json_util::u64_array(&self.seen_errors),
            json_util::u64_array(&es_fp),
            json_util::str_array(&es_text),
            json_util::str_array(&self.seen_git_refs),
            self.tokens_bash,
            self.tokens_read,
            self.tokens_grep,
            self.tokens_other,
            self.reread_count,
            self.exact_dedup_hits,
            self.fuzzy_dedup_hits,
            self.summarize_triggers,
            self.intensity_ultra_calls,
            self.agent_spawns,
            self.agent_estimated_tokens,
            json_util::u64_array(&as_call_n),
            json_util::str_array(&as_tool),
            json_util::u64_array(&as_tokens),
            json_util::u64_array(&as_ts),
            json_util::u64_array(&bw_call_n),
            json_util::u64_array(&bw_tokens),
            json_util::u64_array(&bw_ts),
        )
    }

    pub fn from_json(s: &str) -> Self {
        let map = json_util::extract_all(s);
        let mut c = Self::default();
        c.session_file = json_util::map_str(&map, "session_file").unwrap_or_default();
        c.call_counter = json_util::map_u64(&map, "call_counter").unwrap_or(0);

        let cl_n = json_util::map_u64_array(&map, "call_log_n");
        let cl_cmd = json_util::map_str_array(&map, "call_log_cmd");
        let cl_hash = json_util::map_u64_array(&map, "call_log_hash");
        let cl_len = json_util::map_u64_array(&map, "call_log_len");
        let cl_short = json_util::map_str_array(&map, "call_log_short");
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

        // Shingles — optional field for backwards compatibility with older
        // context.json files. Inner separator is `;` (see to_json comment).
        // If absent or shorter than call_log, missing entries are left as
        // empty Vec and lookup_similar will skip them.
        let cl_sh_strs = json_util::map_str_array(&map, "call_log_shingles");
        for raw in cl_sh_strs.iter().take(n) {
            if raw.is_empty() {
                c.call_log_shingles.push(Vec::new());
            } else {
                let parsed: Vec<u64> =
                    raw.split(';').filter_map(|t| t.parse::<u64>().ok()).collect();
                c.call_log_shingles.push(parsed);
            }
        }

        let sf_path = json_util::map_str_array(&map, "seen_files_path");
        let sf_size = json_util::map_u64_array(&map, "seen_files_size");
        let sf_last = json_util::map_u64_array(&map, "seen_files_last");
        // Phase 4: access field — optional for backward compat; defaults to Read.
        let sf_access = json_util::map_str_array(&map, "seen_files_access");
        let m = sf_path.len().min(sf_size.len()).min(sf_last.len());
        for i in 0..m {
            let access = sf_access
                .get(i)
                .and_then(|s| s.chars().next())
                .map(FileAccess::from_char)
                .unwrap_or(FileAccess::Read);
            c.seen_files.push(FileFingerprint {
                path: sf_path[i].clone(),
                size_class: sf_size[i] as u32,
                last_seen_call: sf_last[i],
                access,
            });
        }

        c.seen_errors = json_util::map_u64_array(&map, "seen_errors");

        // Phase 2: error snippets — optional for backward compat.
        let es_fp = json_util::map_u64_array(&map, "error_snippet_fp");
        let es_text = json_util::map_str_array(&map, "error_snippet_text");
        let es_n = es_fp.len().min(es_text.len());
        for i in 0..es_n {
            c.error_snippets.push((es_fp[i], es_text[i].clone()));
        }

        c.seen_git_refs = json_util::map_str_array(&map, "seen_git_refs");
        c.tokens_bash = json_util::map_u64(&map, "tokens_bash").unwrap_or(0);
        c.tokens_read = json_util::map_u64(&map, "tokens_read").unwrap_or(0);
        c.tokens_grep = json_util::map_u64(&map, "tokens_grep").unwrap_or(0);
        c.tokens_other = json_util::map_u64(&map, "tokens_other").unwrap_or(0);
        c.reread_count = json_util::map_u64(&map, "reread_count").unwrap_or(0) as u32;

        // Phase 6: stat counters — optional for backward compat.
        c.exact_dedup_hits =
            json_util::map_u64(&map, "exact_dedup_hits").unwrap_or(0) as u32;
        c.fuzzy_dedup_hits =
            json_util::map_u64(&map, "fuzzy_dedup_hits").unwrap_or(0) as u32;
        c.summarize_triggers =
            json_util::map_u64(&map, "summarize_triggers").unwrap_or(0) as u32;
        c.intensity_ultra_calls =
            json_util::map_u64(&map, "intensity_ultra_calls").unwrap_or(0) as u32;

        // Phase 7: token economy — optional for backward compat.
        c.agent_spawns =
            json_util::map_u64(&map, "agent_spawns").unwrap_or(0) as u32;
        c.agent_estimated_tokens =
            json_util::map_u64(&map, "agent_estimated_tokens").unwrap_or(0);

        let as_call_n = json_util::map_u64_array(&map, "agent_spawn_log_call_n");
        let as_tool = json_util::map_str_array(&map, "agent_spawn_log_tool");
        let as_tokens = json_util::map_u64_array(&map, "agent_spawn_log_tokens");
        let as_ts = json_util::map_u64_array(&map, "agent_spawn_log_ts");
        let as_n = as_call_n.len().min(as_tool.len()).min(as_tokens.len()).min(as_ts.len());
        for i in 0..as_n {
            c.agent_spawn_log.push(AgentSpawnEntry {
                call_n: as_call_n[i],
                tool_name: as_tool[i].clone(),
                estimated_tokens: as_tokens[i],
                ts: as_ts[i],
            });
        }

        let bw_call_n = json_util::map_u64_array(&map, "burn_window_call_n");
        let bw_tokens = json_util::map_u64_array(&map, "burn_window_tokens");
        let bw_ts = json_util::map_u64_array(&map, "burn_window_ts");
        let bw_n = bw_call_n.len().min(bw_tokens.len()).min(bw_ts.len());
        for i in 0..bw_n {
            c.burn_window.push(BurnEntry {
                call_n: bw_call_n[i],
                tokens: bw_tokens[i],
                ts: bw_ts[i],
            });
        }

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
        assert_eq!(c.call_log.len(), DEFAULT_MAX_CALL_LOG);
        // Oldest entries dropped
        assert_eq!(c.call_log[0].call_n, 9); // calls 1..=8 dropped
    }

    #[test]
    fn lookup_recent_only_within_window() {
        let mut c = SessionContext::default();
        // Record 25 calls: window=16 covers last 16 (calls 10..=25)
        for i in 1..=25u64 {
            c.next_call_n();
            c.record_call(&format!("c{}", i), i * 10, i as usize, i);
        }
        // Last call hash present (call 25, hash=250, len=25)
        assert!(c.lookup_recent(250, 25).is_some());
        // Call 9 is outside window (window starts at call 10)
        assert!(c.lookup_recent(90, 9).is_none());
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
    fn from_json_roundtrip_extract_all() {
        // Build a context, serialize, deserialize with extract_all-based from_json,
        // and verify all fields round-trip correctly.
        let mut c = SessionContext::default();
        c.session_file = "2026-04-19-12.jsonl".to_string();
        c.call_counter = 7;
        c.tokens_bash = 500;
        c.tokens_read = 300;
        c.tokens_grep = 100;
        c.tokens_other = 50;
        c.reread_count = 2;
        c.exact_dedup_hits = 1;
        c.fuzzy_dedup_hits = 3;
        c.summarize_triggers = 2;
        c.intensity_ultra_calls = 1;
        c.agent_spawns = 1;
        c.agent_estimated_tokens = 1000;
        c.note_files(&["/a.rs".to_string(), "/b.rs".to_string()]);
        c.note_errors(&["error: missing field".to_string()]);
        c.note_git(&["abc1234def".to_string()]);
        let n = c.next_call_n();
        c.record_call("cargo test", 0xbeef, 200, n);

        let json = c.to_json();
        let r = SessionContext::from_json(&json);

        assert_eq!(r.session_file, c.session_file);
        assert_eq!(r.call_counter, c.call_counter);
        assert_eq!(r.tokens_bash, c.tokens_bash);
        assert_eq!(r.tokens_read, c.tokens_read);
        assert_eq!(r.tokens_grep, c.tokens_grep);
        assert_eq!(r.tokens_other, c.tokens_other);
        assert_eq!(r.reread_count, c.reread_count);
        assert_eq!(r.exact_dedup_hits, c.exact_dedup_hits);
        assert_eq!(r.fuzzy_dedup_hits, c.fuzzy_dedup_hits);
        assert_eq!(r.call_log.len(), c.call_log.len());
        assert_eq!(r.seen_files.len(), c.seen_files.len());
        assert_eq!(r.seen_errors.len(), c.seen_errors.len());
        assert_eq!(r.seen_git_refs.len(), c.seen_git_refs.len());
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
