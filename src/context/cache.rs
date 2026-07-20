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
    // ── Nudges (auto-curation, item 1) ─────────────────────────────────
    /// Per-session recurrence counters keyed by error fingerprint.
    /// Parallel to `error_count_n`; persisted across sub-process invocations
    /// within the same session via context.json.
    pub error_count_fp: Vec<u64>,
    pub error_count_n: Vec<u32>,
    /// Per-session write/create counts for file paths (parallel arrays).
    pub file_mod_path: Vec<String>,
    pub file_mod_n: Vec<u32>,
    /// Per-session repeat counts for expensive shell command names (parallel arrays).
    pub cmd_repeat_name: Vec<String>,
    pub cmd_repeat_n: Vec<u32>,
    /// Nudge keys already emitted this session — prevents duplicate hints.
    /// Format: `err:<hex>`, `file:<path>`, `cmd:<name>`.
    pub nudged_keys: Vec<String>,
    // ── Skill-injection dedup (session-long) ───────────────────────────────
    /// FNV-1a-64 fingerprints of skill bodies injected this session. Unlike
    /// `call_log` (capped at `max_call_log`, searched within `recent_window`),
    /// this store is unbounded and never windowed — skill re-injections recur
    /// at arbitrary distance and must dedup across the whole session.
    /// Parallel to `skill_inject_call`.
    pub skill_inject_fp: Vec<u64>,
    /// `call_n` of the FIRST injection for each fingerprint (parallel to
    /// `skill_inject_fp`). Referenced in the `identical to Skill #N` note.
    pub skill_inject_call: Vec<u64>,
    // ── Real-context tracking (transcript audit CF-1) ──────────────────────
    /// Effective context tokens of the latest API turn, measured from the
    /// host transcript's `message.usage` (input + cache_read + cache_creation).
    /// 0 = never observed. Feeds adaptive intensity and burn-rate math so
    /// they track the real context, not just squeez-processed bytes.
    pub real_ctx_tokens: u64,
    /// Context window (tokens) of the host model, detected from the transcript's
    /// `model` id (e.g. `[1m]` → 1_000_000, else 200_000). 0 = never observed.
    /// Budget/pressure math keys off this so squeez warns against the real
    /// window instead of the legacy 112.5K default. `context_window_tokens`
    /// config still overrides it.
    pub real_ctx_window: u64,
    /// `cache_read_input_tokens` of the latest measured API turn. Used to
    /// compute the cache-read:I/O ratio for context-leak detection (G1).
    /// 0 = not yet measured.
    pub real_cache_read_tokens: u64,
    // ── Call-rate spike detection (G2) ──────────────────────────────────────
    /// How many track_result calls have been observed in the current 60s window.
    pub calls_this_minute: u32,
    /// Unix timestamp of the start of the current 60-second measurement window.
    pub calls_minute_ts: u64,
    // ── Pending warnings (cross-invocation channel) ─────────────────────────
    /// Warnings queued by observer paths that have no stdout channel into the
    /// model's context (track-result, SubagentStop). Drained and printed as
    /// `[squeez: …]` lines by the next `squeez wrap` invocation. Stored
    /// bracket-free; brackets are added at drain time (the hand-rolled
    /// str_array parser uses `]` as a terminator).
    pub pending_warnings: Vec<String>,
    // ── Image dedup (transcript audit item 3) ───────────────────────────────
    /// FNV-1a-64 fingerprints of image payloads (base64 data) seen this
    /// session. Session-long like the skill store: identical screenshots and
    /// image re-reads recur at arbitrary distance, and the text-shingle path
    /// cannot match base64. Parallel to `image_call`.
    pub image_fp: Vec<u64>,
    /// `call_n` of the FIRST sighting for each image fingerprint.
    pub image_call: Vec<u64>,
    // ── Repeated-screenshot advisory (S3.2) ─────────────────────────────────
    /// FNV-1a-64 of the normalized URL (host + path, no query/fragment) for each
    /// browser screenshot/navigate seen this session. Parallel to `shot_url_ts`.
    /// Never touches image bytes — this is a pure policy nudge to prefer
    /// read_page over re-screenshotting an already-loaded page.
    pub shot_url_fp: Vec<u64>,
    /// Unix timestamp of the most recent screenshot for each `shot_url_fp`.
    pub shot_url_ts: Vec<u64>,
    // ── Workflow abuse prevention ────────────────────────────────────────────
    /// Unix timestamp of the last track_result call. Used to detect idle
    /// periods where the 5-min ephemeral cache may have expired. 0 = no
    /// activity recorded yet (skip warning on fresh session).
    pub last_activity_ts: u64,
    /// Files seen per sub-agent: parallel arrays of agent_id and `;`-joined
    /// file-path lists. Bounded at MAX_AGENT_SPAWN_LOG (16) entries; oldest
    /// entry is evicted when the cap is reached. Used to detect cross-agent
    /// duplicate reads across sibling agents in the same workflow.
    pub subagent_file_map_ids: Vec<String>,
    pub subagent_file_map_paths: Vec<String>, // `;`-joined paths per entry
    // ── Header tag dedup (E1) ────────────────────────────────────────────────
    /// Last emitted `[budget: ...]` tag text (empty = none emitted yet).
    pub last_budget_tag: String,
    /// `call_n` at which `last_budget_tag` was last actually printed.
    pub last_budget_tag_call_n: u64,
    /// Last emitted `[agents: ...]` tag text.
    pub last_agent_tag: String,
    /// `call_n` at which `last_agent_tag` was last actually printed.
    pub last_agent_tag_call_n: u64,
    // ── Flag-forcing escape memo (E3) ────────────────────────────────────────
    /// Base command strings whose arg-tier flag-forced variant failed once
    /// (e.g. an injected `--json` the tool didn't recognize) -- session-long
    /// like the skill store, so a repeated call never re-attempts the same
    /// doomed injection. Bounded at MAX_FLAG_FORCE_FAILED entries.
    pub flag_force_failed: Vec<String>,
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
            error_count_fp: Vec::new(),
            error_count_n: Vec::new(),
            file_mod_path: Vec::new(),
            file_mod_n: Vec::new(),
            cmd_repeat_name: Vec::new(),
            cmd_repeat_n: Vec::new(),
            nudged_keys: Vec::new(),
            skill_inject_fp: Vec::new(),
            skill_inject_call: Vec::new(),
            real_ctx_tokens: 0,
            real_ctx_window: 0,
            real_cache_read_tokens: 0,
            calls_this_minute: 0,
            calls_minute_ts: 0,
            pending_warnings: Vec::new(),
            image_fp: Vec::new(),
            shot_url_fp: Vec::new(),
            shot_url_ts: Vec::new(),
            image_call: Vec::new(),
            last_activity_ts: 0,
            subagent_file_map_ids: Vec::new(),
            subagent_file_map_paths: Vec::new(),
            last_budget_tag: String::new(),
            last_budget_tag_call_n: 0,
            last_agent_tag: String::new(),
            last_agent_tag_call_n: 0,
            flag_force_failed: Vec::new(),
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

    /// Session-long lookup for a previously-injected skill body, keyed by its
    /// FNV-1a-64 fingerprint. Returns the `call_n` of the first injection on an
    /// exact hit. Unlike `lookup_recent`, this scans the entire session (no
    /// `recent_window` bound) because skill re-injections recur far apart.
    pub fn skill_dedup_lookup(&self, fp: u64) -> Option<u64> {
        self.skill_inject_fp
            .iter()
            .position(|&f| f == fp)
            .map(|i| self.skill_inject_call[i])
    }

    /// Record a skill body fingerprint as injected this session and return the
    /// assigned `call_n`. Caller should only invoke after `skill_dedup_lookup`
    /// returned `None` (first injection).
    pub fn skill_dedup_record(&mut self, fp: u64) -> u64 {
        let call_n = self.next_call_n();
        self.skill_inject_fp.push(fp);
        self.skill_inject_call.push(call_n);
        call_n
    }

    /// Session-long lookup for a previously-seen image payload by fingerprint.
    /// Returns the `call_n` of the first sighting on an exact hit.
    pub fn image_dedup_lookup(&self, fp: u64) -> Option<u64> {
        self.image_fp
            .iter()
            .position(|&f| f == fp)
            .map(|i| self.image_call[i])
    }

    /// Record an image payload fingerprint and return the assigned `call_n`.
    pub fn image_dedup_record(&mut self, fp: u64) -> u64 {
        let call_n = self.next_call_n();
        self.image_fp.push(fp);
        self.image_call.push(call_n);
        call_n
    }

    /// Record a browser screenshot of `url` at time `now`. Returns
    /// `Some(elapsed_secs)` exactly once per URL per session when the same page
    /// is screenshotted again within `window_secs` of the previous shot — the
    /// caller turns that into a one-shot advisory to prefer `read_page` over
    /// re-capturing an already-loaded page. Never touches image bytes.
    /// `window_secs == 0` disables the advisory. The URL store is capped.
    pub fn record_screenshot(&mut self, url: &str, now: u64, window_secs: u64) -> Option<u64> {
        const MAX_SHOT_URLS: usize = 64;
        let fp = normalize_url_fp(url);
        if let Some(i) = self.shot_url_fp.iter().position(|&f| f == fp) {
            let prev = self.shot_url_ts[i];
            self.shot_url_ts[i] = now;
            let elapsed = now.saturating_sub(prev);
            if window_secs > 0 && elapsed <= window_secs && self.mark_nudged(&format!("shot:{fp}")) {
                return Some(elapsed);
            }
            None
        } else {
            if self.shot_url_fp.len() >= MAX_SHOT_URLS {
                self.shot_url_fp.remove(0);
                self.shot_url_ts.remove(0);
            }
            self.shot_url_fp.push(fp);
            self.shot_url_ts.push(now);
            None
        }
    }

    /// Queue a warning for the next `squeez wrap` invocation to print.
    /// `msg` must be bracket-free (brackets are added at drain time).
    /// Deduplicates exact repeats so a hot path can't flood the queue.
    pub fn queue_warning(&mut self, msg: &str) {
        let clean: String = msg
            .chars()
            .map(|c| match c {
                '[' => '(',
                ']' => ')',
                _ => c,
            })
            .collect();
        if !self.pending_warnings.iter().any(|w| w == &clean) {
            self.pending_warnings.push(clean);
        }
    }

    /// Drain queued warnings, formatted as `[squeez: …]` lines.
    pub fn drain_warnings(&mut self) -> Vec<String> {
        self.pending_warnings
            .drain(..)
            .map(|w| format!("[squeez: {}]", w))
            .collect()
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

    // ── Nudge counter helpers (item 1) ───────────────────────────────────

    /// Bump the recurrence count for an error fingerprint. Returns the new count.
    pub fn bump_error_count(&mut self, fp: u64) -> u32 {
        if let Some(idx) = self.error_count_fp.iter().position(|f| *f == fp) {
            let n = self.error_count_n[idx].saturating_add(1);
            self.error_count_n[idx] = n;
            n
        } else {
            self.error_count_fp.push(fp);
            self.error_count_n.push(1);
            1
        }
    }

    /// Bump the modification count for a file path. Returns the new count.
    pub fn bump_file_mod(&mut self, path: &str) -> u32 {
        if let Some(idx) = self.file_mod_path.iter().position(|p| p == path) {
            let n = self.file_mod_n[idx].saturating_add(1);
            self.file_mod_n[idx] = n;
            n
        } else {
            self.file_mod_path.push(path.to_string());
            self.file_mod_n.push(1);
            1
        }
    }

    /// Bump the repeat count for a shell command name. Returns the new count.
    pub fn bump_cmd_repeat(&mut self, cmd_name: &str) -> u32 {
        if let Some(idx) = self.cmd_repeat_name.iter().position(|c| c == cmd_name) {
            let n = self.cmd_repeat_n[idx].saturating_add(1);
            self.cmd_repeat_n[idx] = n;
            n
        } else {
            self.cmd_repeat_name.push(cmd_name.to_string());
            self.cmd_repeat_n.push(1);
            1
        }
    }

    /// Update the 60-second rolling call counter and return the current calls/min.
    /// Resets the window when >60s have elapsed since `calls_minute_ts`.
    pub fn tick_call_rate(&mut self) -> u32 {
        let now = crate::session::unix_now();
        if self.calls_minute_ts == 0 || now.saturating_sub(self.calls_minute_ts) >= 60 {
            self.calls_minute_ts = now;
            self.calls_this_minute = 1;
        } else {
            self.calls_this_minute = self.calls_this_minute.saturating_add(1);
        }
        self.calls_this_minute
    }

    /// Mark a nudge key as already emitted. Returns true if newly inserted
    /// (i.e. the caller should print the nudge), false if it was already there.
    pub fn mark_nudged(&mut self, key: &str) -> bool {
        if self.nudged_keys.iter().any(|k| k == key) {
            return false;
        }
        self.nudged_keys.push(key.to_string());
        true
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

    /// Record files read by `agent_id` and return any paths that were already
    /// seen by a **different** agent in this session (cross-agent dup reads).
    /// The map is bounded at `MAX_AGENT_SPAWN_LOG` entries; the oldest is evicted
    /// when full. Empty `agent_id` is a no-op (returns empty).
    pub fn note_subagent_files(&mut self, agent_id: &str, paths: &[String]) -> Vec<String> {
        if agent_id.is_empty() || paths.is_empty() {
            return Vec::new();
        }
        // Collect dups: paths already recorded by any different agent.
        let dups: Vec<String> = paths
            .iter()
            .filter(|p| {
                self.subagent_file_map_ids
                    .iter()
                    .zip(self.subagent_file_map_paths.iter())
                    .any(|(id, joined)| {
                        id != agent_id
                            && joined.split(';').any(|f| f == p.as_str())
                    })
            })
            .cloned()
            .collect();

        // Merge into existing entry or create a new one.
        if let Some(idx) = self.subagent_file_map_ids.iter().position(|id| id == agent_id) {
            let existing = &mut self.subagent_file_map_paths[idx];
            for p in paths {
                let already = existing.split(';').any(|f| f == p.as_str());
                if !already {
                    if !existing.is_empty() {
                        existing.push(';');
                    }
                    existing.push_str(p);
                }
            }
        } else {
            if self.subagent_file_map_ids.len() >= MAX_AGENT_SPAWN_LOG {
                self.subagent_file_map_ids.remove(0);
                self.subagent_file_map_paths.remove(0);
            }
            self.subagent_file_map_ids.push(agent_id.to_string());
            self.subagent_file_map_paths.push(paths.join(";"));
        }
        dups
    }

    /// Clear the header tag-dedup memo (budget/agent tags). Called after
    /// `/compact` — the model's context was just rebuilt and no longer holds
    /// whatever tag value it last saw, so the next header should re-emit
    /// both tags regardless of whether the value actually changed.
    pub fn reset_header_tag_memo(&mut self) {
        self.last_budget_tag.clear();
        self.last_budget_tag_call_n = 0;
        self.last_agent_tag.clear();
        self.last_agent_tag_call_n = 0;
    }
}

// ── Header tag dedup (E1) ────────────────────────────────────────────────────

/// Calls between forced refreshes of an unchanged header tag — a repeated
/// `[budget: ...]`/`[agents: ...]` value is pure overhead once the model has
/// already seen it, but a periodic refresher keeps it from vanishing forever
/// if the model's window slides past the original emission.
const TAG_REFRESH_INTERVAL: u64 = 10;

/// Decide whether a header tag segment should be printed this call, updating
/// the memo in place. Returns `false` (never emitted) for an empty `value` —
/// there's nothing to show or memoize. Otherwise returns `true` when `value`
/// differs from the last emission or `TAG_REFRESH_INTERVAL` calls have
/// elapsed since the last emission.
pub fn dedup_header_tag(
    last_value: &mut String,
    last_call_n: &mut u64,
    value: &str,
    call_n: u64,
) -> bool {
    if value.is_empty() {
        return false;
    }
    let changed = value != last_value.as_str();
    let stale = call_n.saturating_sub(*last_call_n) >= TAG_REFRESH_INTERVAL;
    if changed || stale {
        *last_value = value.to_string();
        *last_call_n = call_n;
        true
    } else {
        false
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
                "# squeez hint: {} already in context (Read tool, call #{}) — reuse cached content if possible",
                arg, call_n
            ));
        }
    }
    None
}

// ── Quota / plan-limit error detection ─────────────────────────────────────

/// True when an error message indicates a quota, rate, or plan limit — a
/// class of error that is never transient, so retrying the same tool is
/// guaranteed waste (transcript audit CF-3: three more Figma calls were
/// issued after a hard "tool call limit on the Starter plan" error).
pub fn is_quota_error(s: &str) -> bool {
    let l = s.to_lowercase();
    l.contains("rate limit")
        || l.contains("ratelimit")
        || l.contains("quota")
        || l.contains("call limit")
        || l.contains("usage limit")
        || l.contains("upgrade your plan")
        || l.contains("too many requests")
        || l.contains("plan limit")
}

// ── Screenshot URL normalization (S3.2) ─────────────────────────────────────

/// Strip a URL down to `host/path` (drop scheme, `www.`, query, fragment, and a
/// trailing slash) so `https://app.co/x?t=1#a` and `http://app.co/x` compare
/// equal — a re-screenshot after a benign query/hash change is still a repeat.
pub fn normalize_url_path(url: &str) -> String {
    let u = url.trim();
    let after_scheme = u.split("://").nth(1).unwrap_or(u);
    let no_query = after_scheme
        .split(['?', '#'])
        .next()
        .unwrap_or(after_scheme);
    let no_www = no_query.strip_prefix("www.").unwrap_or(no_query);
    no_www.trim_end_matches('/').to_string()
}

/// FNV-1a-64 of the normalized `host/path` — the per-URL screenshot key.
pub fn normalize_url_fp(url: &str) -> u64 {
    crate::context::hash::fnv1a_64(normalize_url_path(url).as_bytes())
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

        // Nudge counters as parallel arrays. u32 promoted to u64 for the
        // existing array helper (lossless within u32 range).
        let ec_n_u64: Vec<u64> = self.error_count_n.iter().map(|&n| n as u64).collect();
        let fm_n_u64: Vec<u64> = self.file_mod_n.iter().map(|&n| n as u64).collect();
        let cr_n_u64: Vec<u64> = self.cmd_repeat_n.iter().map(|&n| n as u64).collect();

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
\"burn_window_call_n\":{},\"burn_window_tokens\":{},\"burn_window_ts\":{},\
\"error_count_fp\":{},\"error_count_n\":{},\
\"file_mod_path\":{},\"file_mod_n\":{},\
\"cmd_repeat_name\":{},\"cmd_repeat_n\":{},\
\"nudged_keys\":{},\
\"skill_inject_fp\":{},\"skill_inject_call\":{},\
\"real_ctx_tokens\":{},\"real_ctx_window\":{},\"real_cache_read_tokens\":{},\"calls_this_minute\":{},\"calls_minute_ts\":{},\"pending_warnings\":{},\
\"image_fp\":{},\"image_call\":{},\
\"shot_url_fp\":{},\"shot_url_ts\":{},\
\"last_activity_ts\":{},\"subagent_file_map_ids\":{},\"subagent_file_map_paths\":{},\
\"last_budget_tag\":\"{}\",\"last_budget_tag_call_n\":{},\"last_agent_tag\":\"{}\",\"last_agent_tag_call_n\":{},\
\"flag_force_failed\":{}}}",
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
            json_util::u64_array(&self.error_count_fp),
            json_util::u64_array(&ec_n_u64),
            json_util::str_array(&self.file_mod_path),
            json_util::u64_array(&fm_n_u64),
            json_util::str_array(&self.cmd_repeat_name),
            json_util::u64_array(&cr_n_u64),
            json_util::str_array(&self.nudged_keys),
            json_util::u64_array(&self.skill_inject_fp),
            json_util::u64_array(&self.skill_inject_call),
            self.real_ctx_tokens,
            self.real_ctx_window,
            self.real_cache_read_tokens,
            self.calls_this_minute,
            self.calls_minute_ts,
            json_util::str_array(&self.pending_warnings),
            json_util::u64_array(&self.image_fp),
            json_util::u64_array(&self.image_call),
            json_util::u64_array(&self.shot_url_fp),
            json_util::u64_array(&self.shot_url_ts),
            self.last_activity_ts,
            json_util::str_array(&self.subagent_file_map_ids),
            json_util::str_array(&self.subagent_file_map_paths),
            json_util::escape_str(&self.last_budget_tag),
            self.last_budget_tag_call_n,
            json_util::escape_str(&self.last_agent_tag),
            self.last_agent_tag_call_n,
            json_util::str_array(&self.flag_force_failed),
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

        // Nudge counters — optional for backward compat with older context.json.
        let ec_fp = json_util::map_u64_array(&map, "error_count_fp");
        let ec_n = json_util::map_u64_array(&map, "error_count_n");
        let ec_len = ec_fp.len().min(ec_n.len());
        c.error_count_fp = ec_fp.iter().take(ec_len).copied().collect();
        c.error_count_n = ec_n.iter().take(ec_len).map(|&n| n as u32).collect();

        let fm_path = json_util::map_str_array(&map, "file_mod_path");
        let fm_n = json_util::map_u64_array(&map, "file_mod_n");
        let fm_len = fm_path.len().min(fm_n.len());
        c.file_mod_path = fm_path.iter().take(fm_len).cloned().collect();
        c.file_mod_n = fm_n.iter().take(fm_len).map(|&n| n as u32).collect();

        let cr_name = json_util::map_str_array(&map, "cmd_repeat_name");
        let cr_n = json_util::map_u64_array(&map, "cmd_repeat_n");
        let cr_len = cr_name.len().min(cr_n.len());
        c.cmd_repeat_name = cr_name.iter().take(cr_len).cloned().collect();
        c.cmd_repeat_n = cr_n.iter().take(cr_len).map(|&n| n as u32).collect();

        c.nudged_keys = json_util::map_str_array(&map, "nudged_keys");

        // Skill-injection dedup store — optional for backward compat.
        let si_fp = json_util::map_u64_array(&map, "skill_inject_fp");
        let si_call = json_util::map_u64_array(&map, "skill_inject_call");
        let si_len = si_fp.len().min(si_call.len());
        c.skill_inject_fp = si_fp.iter().take(si_len).copied().collect();
        c.skill_inject_call = si_call.iter().take(si_len).copied().collect();

        // Real-context + pending warnings + image dedup — optional for
        // backward compat with older context.json files.
        c.real_ctx_tokens = json_util::map_u64(&map, "real_ctx_tokens").unwrap_or(0);
        c.real_ctx_window = json_util::map_u64(&map, "real_ctx_window").unwrap_or(0);
        c.real_cache_read_tokens = json_util::map_u64(&map, "real_cache_read_tokens").unwrap_or(0);
        c.calls_this_minute = json_util::map_u64(&map, "calls_this_minute").unwrap_or(0) as u32;
        c.calls_minute_ts = json_util::map_u64(&map, "calls_minute_ts").unwrap_or(0);
        c.pending_warnings = json_util::map_str_array(&map, "pending_warnings");
        let im_fp = json_util::map_u64_array(&map, "image_fp");
        let im_call = json_util::map_u64_array(&map, "image_call");
        let im_len = im_fp.len().min(im_call.len());
        c.image_fp = im_fp.iter().take(im_len).copied().collect();
        c.image_call = im_call.iter().take(im_len).copied().collect();
        // Repeated-screenshot advisory (S3.2) — optional for backward compat;
        // absent keys deserialize to empty, so old context.json loads cleanly.
        let su_fp = json_util::map_u64_array(&map, "shot_url_fp");
        let su_ts = json_util::map_u64_array(&map, "shot_url_ts");
        let su_len = su_fp.len().min(su_ts.len());
        c.shot_url_fp = su_fp.iter().take(su_len).copied().collect();
        c.shot_url_ts = su_ts.iter().take(su_len).copied().collect();

        // Workflow abuse prevention — optional for backward compat.
        c.last_activity_ts = json_util::map_u64(&map, "last_activity_ts").unwrap_or(0);
        let saf_ids = json_util::map_str_array(&map, "subagent_file_map_ids");
        let saf_paths = json_util::map_str_array(&map, "subagent_file_map_paths");
        let saf_len = saf_ids.len().min(saf_paths.len());
        c.subagent_file_map_ids = saf_ids.iter().take(saf_len).cloned().collect();
        c.subagent_file_map_paths = saf_paths.iter().take(saf_len).cloned().collect();

        // Header tag dedup memo (E1) — optional for backward compat.
        c.last_budget_tag = json_util::map_str(&map, "last_budget_tag").unwrap_or_default();
        c.last_budget_tag_call_n =
            json_util::map_u64(&map, "last_budget_tag_call_n").unwrap_or(0);
        c.last_agent_tag = json_util::map_str(&map, "last_agent_tag").unwrap_or_default();
        c.last_agent_tag_call_n =
            json_util::map_u64(&map, "last_agent_tag_call_n").unwrap_or(0);

        // Flag-force escape memo (E3) — optional for backward compat.
        c.flag_force_failed = json_util::map_str_array(&map, "flag_force_failed");

        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_dedup_lookup_and_record() {
        let mut c = SessionContext::default();
        assert_eq!(c.skill_dedup_lookup(0xABCD), None);
        let call_n = c.skill_dedup_record(0xABCD);
        assert_eq!(c.skill_dedup_lookup(0xABCD), Some(call_n));
        // A different fingerprint is independent.
        assert_eq!(c.skill_dedup_lookup(0x1234), None);
    }

    #[test]
    fn skill_dedup_survives_json_roundtrip() {
        let mut c = SessionContext::default();
        let call_n = c.skill_dedup_record(0xDEADBEEF);
        let restored = SessionContext::from_json(&c.to_json());
        assert_eq!(restored.skill_dedup_lookup(0xDEADBEEF), Some(call_n));
    }

    // ── Header tag dedup (E1) ────────────────────────────────────────────────

    #[test]
    fn dedup_header_tag_suppresses_unchanged_value() {
        let mut last = String::new();
        let mut last_n = 0u64;
        // First emission: empty → value is a change, must show.
        assert!(dedup_header_tag(&mut last, &mut last_n, "[budget: ~10 calls left]", 1));
        assert_eq!(last, "[budget: ~10 calls left]");
        assert_eq!(last_n, 1);
        // Second call, unchanged value, within the refresh window: suppressed.
        assert!(!dedup_header_tag(&mut last, &mut last_n, "[budget: ~10 calls left]", 2));
        assert_eq!(last_n, 1, "memo must not move on a suppressed call");
        // Value changed: reappears.
        assert!(dedup_header_tag(&mut last, &mut last_n, "[budget: ~9 calls left]", 3));
        assert_eq!(last, "[budget: ~9 calls left]");
        assert_eq!(last_n, 3);
    }

    #[test]
    fn dedup_header_tag_refreshes_after_interval() {
        let mut last = "[budget: ~10 calls left]".to_string();
        let mut last_n = 1u64;
        // Same value, but TAG_REFRESH_INTERVAL calls have elapsed — the
        // periodic refresher forces re-emission even though unchanged.
        assert!(dedup_header_tag(&mut last, &mut last_n, "[budget: ~10 calls left]", 11));
        assert_eq!(last_n, 11);
    }

    #[test]
    fn dedup_header_tag_empty_value_never_emitted() {
        let mut last = String::new();
        let mut last_n = 0u64;
        assert!(!dedup_header_tag(&mut last, &mut last_n, "", 5));
        assert_eq!(last_n, 0, "empty value must not be memoized as emitted");
    }

    #[test]
    fn reset_header_tag_memo_clears_state() {
        let mut ctx = SessionContext::default();
        ctx.last_budget_tag = "[budget: ~5 calls left]".to_string();
        ctx.last_budget_tag_call_n = 7;
        ctx.last_agent_tag = "[agents: 2 calls]".to_string();
        ctx.last_agent_tag_call_n = 7;
        ctx.reset_header_tag_memo();
        assert!(ctx.last_budget_tag.is_empty());
        assert_eq!(ctx.last_budget_tag_call_n, 0);
        assert!(ctx.last_agent_tag.is_empty());
        assert_eq!(ctx.last_agent_tag_call_n, 0);
    }

    #[test]
    fn header_tag_memo_survives_json_roundtrip() {
        let mut c = SessionContext::default();
        c.last_budget_tag = "[budget: ~5 calls left]".to_string();
        c.last_budget_tag_call_n = 12;
        c.last_agent_tag = "[agents: 3 calls, ~900K est. tokens]".to_string();
        c.last_agent_tag_call_n = 12;
        let restored = SessionContext::from_json(&c.to_json());
        assert_eq!(restored.last_budget_tag, c.last_budget_tag);
        assert_eq!(restored.last_budget_tag_call_n, c.last_budget_tag_call_n);
        assert_eq!(restored.last_agent_tag, c.last_agent_tag);
        assert_eq!(restored.last_agent_tag_call_n, c.last_agent_tag_call_n);
    }

    #[test]
    fn image_dedup_lookup_and_roundtrip() {
        let mut c = SessionContext::default();
        assert_eq!(c.image_dedup_lookup(0x1111), None);
        let call_n = c.image_dedup_record(0x1111);
        assert_eq!(c.image_dedup_lookup(0x1111), Some(call_n));
        let restored = SessionContext::from_json(&c.to_json());
        assert_eq!(restored.image_dedup_lookup(0x1111), Some(call_n));
    }

    #[test]
    fn real_ctx_tokens_survives_roundtrip() {
        let mut c = SessionContext::default();
        c.real_ctx_tokens = 286_129;
        c.real_ctx_window = 1_000_000;
        let restored = SessionContext::from_json(&c.to_json());
        assert_eq!(restored.real_ctx_tokens, 286_129);
        assert_eq!(restored.real_ctx_window, 1_000_000);
    }

    #[test]
    fn queue_warning_sanitizes_dedups_and_drains() {
        let mut c = SessionContext::default();
        c.queue_warning("limit hit [tool] retry");
        c.queue_warning("limit hit [tool] retry"); // exact repeat → dropped
        c.queue_warning("other warning");
        // Brackets sanitized so the str_array parser round-trips safely.
        let restored = SessionContext::from_json(&c.to_json());
        assert_eq!(restored.pending_warnings.len(), 2);
        let mut r = restored;
        let drained = r.drain_warnings();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0], "[squeez: limit hit (tool) retry]");
        assert!(r.pending_warnings.is_empty());
    }

    #[test]
    fn quota_error_detection() {
        assert!(is_quota_error(
            "You've reached the Figma MCP tool call limit on the Starter plan. Upgrade your plan for more."
        ));
        assert!(is_quota_error("429 Too Many Requests"));
        assert!(is_quota_error("API rate limit exceeded"));
        assert!(is_quota_error("monthly quota exhausted"));
        assert!(!is_quota_error("error: cannot find symbol"));
        assert!(!is_quota_error("test result: ok. 5 passed"));
    }

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

    #[test]
    fn normalize_url_path_strips_scheme_query_and_www() {
        assert_eq!(normalize_url_path("https://app.co/x?t=1#a"), "app.co/x");
        assert_eq!(normalize_url_path("http://app.co/x"), "app.co/x");
        assert_eq!(normalize_url_path("https://www.app.co/x/"), "app.co/x");
        // Benign query/hash change compares equal.
        assert_eq!(normalize_url_fp("https://app.co/x?a=1"), normalize_url_fp("http://app.co/x#top"));
    }

    #[test]
    fn record_screenshot_warns_once_within_window() {
        let mut c = SessionContext::default();
        // First visit: no warning, just records.
        assert_eq!(c.record_screenshot("https://app.co/dash", 1000, 300), None);
        // Repeat within window → warn once, elapsed reported.
        assert_eq!(c.record_screenshot("https://app.co/dash?x=1", 1100, 300), Some(100));
        // Second repeat: same URL, still within window, but already nudged → silent.
        assert_eq!(c.record_screenshot("https://app.co/dash", 1150, 300), None);
        // A different URL is independent.
        assert_eq!(c.record_screenshot("https://app.co/other", 1200, 300), None);
    }

    #[test]
    fn record_screenshot_silent_outside_window_and_when_disabled() {
        let mut c = SessionContext::default();
        assert_eq!(c.record_screenshot("https://app.co/p", 1000, 300), None);
        // 400s later — outside the 300s window → no warning.
        assert_eq!(c.record_screenshot("https://app.co/p", 1400, 300), None);
        // window_secs=0 disables entirely.
        let mut d = SessionContext::default();
        assert_eq!(d.record_screenshot("https://app.co/p", 1000, 0), None);
        assert_eq!(d.record_screenshot("https://app.co/p", 1010, 0), None);
    }

    #[test]
    fn old_context_json_without_shot_fields_loads_clean() {
        // Backward-compat: a context.json written before S3.2 has no shot_url_*
        // keys; it must deserialize with empty arrays and no panic.
        let json = r#"{"session_file":"s.jsonl","call_counter":2,"image_fp":[1],"image_call":[1]}"#;
        let c = SessionContext::from_json(json);
        assert_eq!(c.call_counter, 2);
        assert!(c.shot_url_fp.is_empty());
        assert!(c.shot_url_ts.is_empty());
    }
}
