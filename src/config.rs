use crate::commands::focus::Focus;
use crate::commands::persona::Persona;

#[derive(Debug, Clone)]
pub struct Config {
    pub enabled: bool,
    /// Header visibility: "always" | "net" | "off". `net` (default) follows
    /// the net-win gate (R4) — header suppressed when compression saved less
    /// than `net_win_min_tokens`. `always` prints it unconditionally (gate
    /// still governs savings accounting, just not header visibility). `off`
    /// never prints it; follow-up markers/warnings still print.
    pub show_header: String,
    pub max_lines: usize,
    pub dedup_min: usize,
    pub git_log_max_commits: usize,
    pub git_diff_max_lines: usize,
    pub docker_logs_max_lines: usize,
    pub find_max_results: usize,
    pub bypass: Vec<String>,
    pub compact_threshold_tokens: u64,
    pub memory_retention_days: u32,
    // ── Context engine flags ────────────────────────────────────────────
    pub adaptive_intensity: bool,
    pub context_cache_enabled: bool,
    pub redundancy_cache_enabled: bool,
    /// Session-long, path-keyed dedup of repeated `Read` output. Independent
    /// of `redundancy_cache_enabled` only in the off direction: it requires
    /// that flag too. Kill-switch for the store without a rebuild.
    pub read_dedup_session_long: bool,
    pub summarize_threshold_lines: usize,
    // ── Output / memory-file flags ──────────────────────────────────────
    pub persona: Persona,
    /// Output *structure* axis, orthogonal to `persona` (which sets terseness):
    /// `adhd` shapes banner, advisories and the model's prose next-action-first.
    pub focus: Focus,
    pub auto_compress_md: bool,
    pub lang: String,
    // ── Token economy (phase 7) ───────────────────────────────────────────
    /// Fraction of budget at which sub-agent cost triggers a warning (default 0.50).
    pub agent_warn_threshold_pct: f32,
    /// Predict pressure warning when calls remaining < this (default 20).
    pub burn_rate_warn_calls: u64,
    /// Estimated tokens per sub-agent spawn (default 200_000).
    pub agent_spawn_cost: u64,

    /// Escalate compression when this call's output will be re-read many times.
    /// See `context::intensity::amplification_level` for why this is keyed on
    /// demonstrated longevity + remaining headroom rather than on "early calls".
    pub amplification_aware: bool,
    /// Calls the session must already have run before amplification escalates.
    pub amplification_min_calls: u64,
    /// Headroom (predicted calls before compaction) required to escalate.
    pub amplification_min_remaining: u64,
    /// Median tokens/call below which amplification is not worth fidelity.
    pub amplification_min_call_tokens: u64,
    /// Max lines injected into Read tool_input (0 = disabled, default 0).
    pub read_max_lines: usize,
    /// Max results injected into Grep tool_input (0 = disabled, default 0).
    pub grep_max_results: usize,
    // ── Tunables (phase 5) ──────────────────────────────────────────────
    /// Max entries in the rolling call log (default 32).
    pub max_call_log: usize,
    /// How many recent calls are eligible for redundancy lookup (default 16).
    pub recent_window: u64,
    /// Minimum Jaccard similarity for fuzzy redundancy match (default 0.85).
    pub similarity_threshold: f32,
    /// Fraction of context budget at which Full graduates to Ultra (default 0.80).
    pub ultra_trigger_pct: f32,
    /// Default `n` for `squeez_prior_summaries` MCP tool (default 5).
    pub mcp_prior_summaries_default: usize,
    /// Default `n` for `squeez_recent_calls` MCP tool (default 10).
    pub mcp_recent_calls_default: usize,
    /// Calls remaining threshold for State-First Pattern warning (default 10).
    pub state_warn_calls: u64,
    // ── Signature-mode (US-001) ─────────────────────────────────────────────
    /// Enable signature-mode compression for large code files (default true).
    pub sig_mode_enabled: bool,
    /// Minimum line count to trigger signature-mode (default 400).
    pub sig_mode_threshold_lines: usize,
    /// Also capture indented method signatures inside impl/class blocks (default false).
    pub sig_mode_include_indented: bool,
    // ── Memory-file size warning (US-002) ───────────────────────────────────
    /// Token threshold above which inject_memory emits a size warning banner
    /// inside the squeez block. Set to 0 to disable. Default: 1000.
    pub memory_file_warn_tokens: usize,
    // ── Structured summary format (US-003) ──────────────────────────────────
    /// Summary output shape: "prose" | "structured" | "auto".
    /// "auto" selects Structured when Intensity == Ultra, Prose otherwise.
    pub summary_format: String,
    // ── Agent prompt compression ─────────────────────────────────────────────
    /// Max token estimate for Agent/Task prompt before compression kicks in.
    /// 0 = disabled. Default: 2000.
    pub agent_prompt_max_tokens: usize,
    /// Pass through all output uncompressed when SQUEEZ_PLAN_MODE=1 env var is set.
    /// Default: true. Set to false to compress even in plan mode.
    pub plan_mode_passthrough: bool,
    // ── Auto-curation nudges (item 1) ────────────────────────────────────────
    /// Emit `[squeez: hint ...]` markers when recurring patterns are detected.
    /// Default: true.
    pub nudge_enabled: bool,
    /// Recurrence count at which a repeated error fingerprint triggers a nudge.
    /// Default: 3.
    pub nudge_error_threshold: u32,
    /// Modification count at which a repeatedly-edited file triggers a nudge.
    /// Default: 5.
    pub nudge_file_mod_threshold: u32,
    /// Repeat count at which an expensive command triggers a nudge.
    /// Default: 4.
    pub nudge_cmd_repeat_threshold: u32,
    // ── Continuous handler calibration (item 2) ──────────────────────────────
    /// Accumulate per-handler in/out token counts in handler_stats.json so
    /// `squeez_handler_stats` can surface under/over-performers. Default: true.
    pub handler_stats_enabled: bool,
    // ── PostToolUse compression for Read/Edit/Write (gap fix) ────────────────
    /// Override `summarize_threshold_lines` when the tool is Read. Currently
    /// inert: `compress_output.rs` exempts Read from destructive summarization
    /// entirely (source files are structured content, not log output — see
    /// the `is_structured_payload` comment there). Kept for `should_apply_for_tool`
    /// callers that do want a Read-specific override. Set to 0 to fall back to
    /// the global `summarize_threshold_lines`.
    pub read_summarize_threshold_lines: usize,
    /// Byte-size trigger for summarize, independent of line count. A single-line
    /// 50 KB JSON blob (e.g. `az`/`curl` output) has `lines.len() == 1` and slips
    /// past every line-based threshold. When total output bytes exceed this, the
    /// summary fires regardless of line count. Default 24576 (24 KB). Set to 0 to
    /// disable the byte trigger. Benign outputs get the same `BENIGN_MULTIPLIER`
    /// relaxation as the line threshold.
    pub summarize_threshold_bytes: usize,
    /// Advisory window for the repeated-screenshot nudge (S3.2). When the same
    /// browser URL is navigated/screenshotted again within this many seconds,
    /// squeez queues a one-shot hint to prefer `read_page` over re-capturing an
    /// already-loaded page. Default 300 (5 min). 0 disables the nudge. Never
    /// touches image bytes.
    pub screenshot_repeat_window_secs: u64,
    /// Strip the verbose `"Here's the result of running cat -n on a snippet of
    /// the edited file:"` preamble from Edit/Write tool results. Default: true.
    pub strip_edit_preamble: bool,
    // ── Real-context tracking (transcript audit CF-1/CF-2) ──────────────────
    /// Host context window in tokens. When > 0 this replaces the
    /// `compact_threshold_tokens * 5/4` budget formula so adaptive intensity
    /// and burn-rate math are keyed to the real window (e.g. 200000, or
    /// 1000000 for a `[1m]` model). 0 = keep the legacy formula. Default: 0.
    pub context_window_tokens: u64,
    /// Seconds `squeez wrap` waits before killing the wrapped command. squeez
    /// wraps every shell command, so this is the ceiling on any legitimately
    /// long one — a full test suite, a release build, a data migration. The
    /// old hardcoded 120s turned those into a bare exit 124. Overridable per
    /// invocation via `SQUEEZ_WRAP_TIMEOUT_SECS`. Default: 120.
    pub wrap_timeout_secs: u64,
    /// Multiplier on the internal token estimate to track a model's tokenizer
    /// density. 1.0 = legacy. Newer Claude tokenizers (e.g. Sonnet 5) pack
    /// ~1.0–1.35× more tokens into the same text, so set ~1.15 there. Budget
    /// math prefers measured context tokens when available, so this only
    /// affects the estimate/fallback paths. Default: 1.0.
    pub tokenizer_scale: f32,
    /// Token threshold above which a sub-agent's returned result triggers a
    /// warning suggesting the file + short-summary return pattern. A 24K-token
    /// plan riding in the prefix for 100 turns costs ~2.4M cache-read tokens.
    /// 0 = disabled. Default: 3000.
    pub subagent_result_warn_tokens: usize,
    /// Occurrence count at which a repeated quota/plan-limit error (e.g.
    /// "rate limit", "upgrade your plan") triggers a stop-retrying warning.
    /// Lower than `nudge_error_threshold` because quota errors are never
    /// transient. 0 = disabled. Default: 2.
    pub quota_error_threshold: u32,
    /// Days after which `squeez doctor` flags handler_stats/tokens_est as
    /// stale relative to newer session activity (tracking dead while sessions
    /// run). 0 = disable the freshness check. Default: 3.
    pub doctor_stale_days: u64,
    // ── Workflow abuse prevention (audit findings) ───────────────────────────
    /// Seconds of inactivity before warning that the 5-min ephemeral cache may
    /// have expired. Warn at 4.5 min so the user can act before the rebuild hits.
    /// 0 = disabled. Default: 270.
    pub cache_idle_warn_secs: u64,
    /// Time window (seconds) for detecting parallel-agent bursts. Default: 120.
    pub parallel_agent_burst_window_secs: u64,
    /// Number of agents spawned within `parallel_agent_burst_window_secs` that
    /// triggers a budget warning. Default: 5.
    pub parallel_agent_burst_threshold: usize,
    /// Cumulative subagent output tokens at which a total-cost warning fires
    /// for the session. 0 = disabled. Default: 50_000.
    pub subagent_total_warn_tokens: u64,
    /// Tool calls per 60-second window that triggers a high-call-rate warning.
    /// Detects runaway agent loops. 0 = disabled. Default: 20.
    pub call_rate_warn_per_min: u32,
    // ── Reversible compression (P1, headroom CCR-style) ──────────────────────
    /// Stash the verbatim original when a large output is compressed so the
    /// model can recover it via the `squeez_retrieve` MCP tool. Default: true.
    pub retrieve_enabled: bool,
    /// Days a stashed blob lives before pruning. 0 = never prune. Default: 7.
    pub retrieve_ttl_days: u64,
    /// Minimum original line count before squeez bothers stashing a blob.
    /// Default: 40.
    pub retrieve_min_lines: usize,
    // ── Log-template compaction (P2) ─────────────────────────────────────────
    /// Collapse near-identical log lines (differing only by timestamps/ids/
    /// numbers) into a single `[×N] <template>` line. Default: true.
    pub log_template_enabled: bool,
    /// Minimum recurrence before a template is collapsed. Default: 3.
    pub log_template_min: usize,
    // ── Relevance-aware truncation (P3) ──────────────────────────────────────
    /// When the generic handler must truncate, keep the highest-relevance lines
    /// (error/signal words + command query terms) instead of a blind head.
    /// Default: true.
    pub relevance_truncation_enabled: bool,
    // ── Content-class calibrated density (R2) ────────────────────────────────
    /// Use the content-class estimator for wrap token accounting: Dense
    /// payloads (JSON/code/diffs) ≈ chars/2, Prose ≈ chars/3.7, Mixed falls
    /// back to the char-class estimate. Calibrated on pxpipe measurements
    /// (~1.91 chars/token dense, ~3.5-3.7 prose). False = flat
    /// `estimate_scaled` path. Default: true.
    pub class_density: bool,
    // ── Net-win gate (R4) ────────────────────────────────────────────────────
    /// Minimum token savings compression must achieve to be worth the
    /// ~15-25-token `# squeez` header. Below this, wrap emits the original
    /// output and suppresses the header (follow-up warnings still print).
    /// 0 = disabled. Default: 24.
    pub net_win_min_tokens: usize,
    /// Run `economy::preservation`'s anchor scan at wrap time on
    /// high-reduction calls (>= 90%). When the surviving-anchor fraction
    /// falls below `preservation_floor`, the verbatim original is stashed
    /// even outside the usual size gates and the header carries
    /// `[anchors: N%]`. Default: true.
    pub preservation_guard: bool,
    /// Surviving-anchor fraction below which a high-reduction call is
    /// treated as over-compressed. Default: 0.70 (matches
    /// `preservation::RISK_PRESERVATION_FLOOR`).
    pub preservation_floor: f32,
    /// Use the shipped filter-DSL rule pack (`assets/filters_builtin.ini`).
    /// User and project `filters.ini` rules always shadow a built-in of the
    /// same name; this switch turns the whole pack off. Default: true.
    pub builtin_filters: bool,
    // ── Bash-wrap safety (#150) ──────────────────────────────────────────────
    /// Master switch for rewriting Bash commands to `squeez wrap '…'` in the
    /// PreToolUse hook. When false, commands run unwrapped (no output
    /// compression) and the host's native permission flow applies untouched.
    /// Default: true.
    pub wrap_bash: bool,
    /// Substring patterns marking a command too risky to wrap. A matching
    /// command is passed through UNWRAPPED with no permission decision, so the
    /// host evaluates the user's deny/ask rules against the *original* command
    /// (e.g. a `Bash(git push *)` deny rule keeps working). A safety net, not a
    /// sandbox — extend it to match your own rules. Default: common destructive
    /// operations.
    pub bash_risk_patterns: Vec<String>,
    // ── Success collapse (E5) ─────────────────────────────────────────────────
    /// When true, a zero-signal successful run (exit 0, no error markers) of
    /// an allow-listed low-signal command (git push/pull/fetch/add/commit,
    /// package installs, docker pull/build, wrangler deploy) collapses to a
    /// single `ok <cmd> (...)` line instead of printing the full output. The
    /// original is always still stashed and retrievable. Default: true.
    pub success_collapse: bool,
    /// Command prefixes to exclude from success collapse even though they
    /// match the built-in allow-list (e.g. a wrapped `git commit` alias that
    /// prints something you rely on). Default: empty.
    pub success_collapse_deny: Vec<String>,
    // ── Flag forcing (E3) ──────────────────────────────────────────────────────
    /// `off` disables both tiers. `env` (default) sets safe formatting env
    /// vars (LC_ALL/GIT_PAGER/NO_COLOR/FORCE_COLOR) via `Command::env()` --
    /// never changes command semantics. `full` additionally appends a
    /// machine-readable-output flag (e.g. `--json`) for runners
    /// `commands::reporters` has a structured parser for, gated on no shell
    /// metacharacters and a one-shot fallback if the tool rejects the flag.
    pub flag_force: String,
    /// Command prefixes to exclude from arg-tier flag forcing even under
    /// `flag_force = full`. Default: empty.
    pub flag_force_deny: Vec<String>,
    // ── Pato-buddy companion ─────────────────────────────────────────────────
    /// Vendored duck companion whose XP grows with squeez net savings
    /// (25k tokens = 1 XP). When false, `squeez setup` strips the buddy hook
    /// entries and the shims exit silently. Default: true.
    pub buddy: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            show_header: "net".to_string(),
            max_lines: 120,
            dedup_min: 2,
            git_log_max_commits: 20,
            git_diff_max_lines: 150,
            docker_logs_max_lines: 100,
            find_max_results: 50,
            bypass: vec![
                "docker exec".to_string(),
                "psql".to_string(),
                "mysql".to_string(),
                "ssh".to_string(),
            ],
            compact_threshold_tokens: 90_000,
            memory_retention_days: 30,
            adaptive_intensity: true,
            context_cache_enabled: true,
            redundancy_cache_enabled: true,
            read_dedup_session_long: true,
            summarize_threshold_lines: 300,
            persona: Persona::Ultra,
            focus: Focus::Off,
            auto_compress_md: true,
            lang: "en".to_string(),
            agent_warn_threshold_pct: 0.50,
            burn_rate_warn_calls: 30,
            agent_spawn_cost: 350_000,
            amplification_aware: true,
            amplification_min_calls: 20,
            amplification_min_remaining: 8,
            amplification_min_call_tokens: 400,
            read_max_lines: 300,
            grep_max_results: 100,
            max_call_log: 32,
            recent_window: 16,
            similarity_threshold: 0.85,
            ultra_trigger_pct: 0.65,
            mcp_prior_summaries_default: 5,
            mcp_recent_calls_default: 10,
            state_warn_calls: 10,
            sig_mode_enabled: true,
            sig_mode_threshold_lines: 400,
            sig_mode_include_indented: false,
            memory_file_warn_tokens: 1000,
            summary_format: "auto".to_string(),
            agent_prompt_max_tokens: 2000,
            plan_mode_passthrough: true,
            nudge_enabled: true,
            nudge_error_threshold: 3,
            nudge_file_mod_threshold: 5,
            nudge_cmd_repeat_threshold: 4,
            handler_stats_enabled: true,
            read_summarize_threshold_lines: 150,
            summarize_threshold_bytes: 24576,
            screenshot_repeat_window_secs: 300,
            strip_edit_preamble: true,
            context_window_tokens: 0,
            wrap_timeout_secs: DEFAULT_WRAP_TIMEOUT_SECS,
            tokenizer_scale: 1.0,
            subagent_result_warn_tokens: 3000,
            quota_error_threshold: 2,
            doctor_stale_days: 3,
            cache_idle_warn_secs: 270,
            parallel_agent_burst_window_secs: 120,
            parallel_agent_burst_threshold: 5,
            subagent_total_warn_tokens: 50_000,
            call_rate_warn_per_min: 20,
            retrieve_enabled: true,
            retrieve_ttl_days: 7,
            retrieve_min_lines: 40,
            log_template_enabled: true,
            log_template_min: 3,
            relevance_truncation_enabled: true,
            class_density: true,
            net_win_min_tokens: 24,
            preservation_guard: true,
            preservation_floor: 0.70,
            builtin_filters: true,
            wrap_bash: true,
            bash_risk_patterns: vec![
                "rm -rf".to_string(),
                "rm -fr".to_string(),
                "rm -r -f".to_string(),
                "git push --force".to_string(),
                "git push -f".to_string(),
                "git reset --hard".to_string(),
                "npm publish".to_string(),
                "yarn publish".to_string(),
                "pnpm publish".to_string(),
                "dd if=".to_string(),
                "mkfs".to_string(),
                "> /dev/sd".to_string(),
            ],
            success_collapse: true,
            success_collapse_deny: Vec::new(),
            flag_force: "env".to_string(),
            flag_force_deny: Vec::new(),
            buddy: true,
        }
    }
}

/// Prior hardcoded `squeez wrap` timeout, kept as the default so existing
/// behaviour is unchanged when nothing is configured.
pub const DEFAULT_WRAP_TIMEOUT_SECS: u64 = 120;

/// Effective wrap timeout. `SQUEEZ_WRAP_TIMEOUT_SECS` wins when it parses to a
/// positive number — the point of the env knob is unblocking one long command
/// without editing a file. Anything unusable (absent, empty, non-numeric,
/// negative, zero) falls through to the config value, and a zero there falls
/// through to the default rather than killing every command instantly.
pub fn resolve_wrap_timeout_secs(cfg_secs: u64, env: Option<&str>) -> u64 {
    if let Some(secs) = env.and_then(|v| v.trim().parse::<u64>().ok()).filter(|s| *s > 0) {
        return secs;
    }
    if cfg_secs > 0 {
        return cfg_secs;
    }
    DEFAULT_WRAP_TIMEOUT_SECS
}

impl Config {
    pub fn from_str(s: &str) -> Self {
        let mut c = Self::default();
        for line in s.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let (k, v) = (k.trim(), v.trim());
                match k {
                    "enabled" => c.enabled = v == "true",
                    "show_header" => {
                        c.show_header = match v {
                            "always" | "net" | "off" => v.to_string(),
                            _ => "net".to_string(),
                        };
                    }
                    "max_lines" => c.max_lines = v.parse().unwrap_or(c.max_lines),
                    "dedup_min" => c.dedup_min = v.parse().unwrap_or(c.dedup_min),
                    "git_log_max_commits" => {
                        c.git_log_max_commits = v.parse().unwrap_or(c.git_log_max_commits)
                    }
                    "git_diff_max_lines" => {
                        c.git_diff_max_lines = v.parse().unwrap_or(c.git_diff_max_lines)
                    }
                    "docker_logs_max_lines" => {
                        c.docker_logs_max_lines = v.parse().unwrap_or(c.docker_logs_max_lines)
                    }
                    "find_max_results" => {
                        c.find_max_results = v.parse().unwrap_or(c.find_max_results)
                    }
                    "bypass" => c.bypass = v.split(',').map(|s| s.trim().to_string()).collect(),
                    "compact_threshold_tokens" => {
                        c.compact_threshold_tokens = v.parse().unwrap_or(c.compact_threshold_tokens)
                    }
                    "memory_retention_days" => {
                        c.memory_retention_days = v.parse().unwrap_or(c.memory_retention_days)
                    }
                    "adaptive_intensity" => c.adaptive_intensity = v == "true",
                    "context_cache_enabled" => c.context_cache_enabled = v == "true",
                    "redundancy_cache_enabled" => c.redundancy_cache_enabled = v == "true",
                    "read_dedup_session_long" => c.read_dedup_session_long = v == "true",
                    "summarize_threshold_lines" => {
                        c.summarize_threshold_lines =
                            v.parse().unwrap_or(c.summarize_threshold_lines)
                    }
                    "persona" => c.persona = crate::commands::persona::from_str(v),
                    "focus" => c.focus = crate::commands::focus::from_str(v),
                    "auto_compress_md" => c.auto_compress_md = v == "true",
                    "lang" => c.lang = v.to_string(),
                    "agent_warn_threshold_pct" => {
                        c.agent_warn_threshold_pct =
                            v.parse().unwrap_or(c.agent_warn_threshold_pct)
                    }
                    "burn_rate_warn_calls" => {
                        c.burn_rate_warn_calls =
                            v.parse().unwrap_or(c.burn_rate_warn_calls)
                    }
                    "agent_spawn_cost" => {
                        c.agent_spawn_cost = v.parse().unwrap_or(c.agent_spawn_cost)
                    }
                    "read_max_lines" => {
                        c.read_max_lines = v.parse().unwrap_or(c.read_max_lines)
                    }
                    "grep_max_results" => {
                        c.grep_max_results = v.parse().unwrap_or(c.grep_max_results)
                    }
                    "max_call_log" => {
                        c.max_call_log = v.parse().unwrap_or(c.max_call_log)
                    }
                    "recent_window" => {
                        c.recent_window = v.parse().unwrap_or(c.recent_window)
                    }
                    "similarity_threshold" => {
                        c.similarity_threshold = v.parse().unwrap_or(c.similarity_threshold)
                    }
                    "ultra_trigger_pct" => {
                        c.ultra_trigger_pct = v.parse().unwrap_or(c.ultra_trigger_pct)
                    }
                    "mcp_prior_summaries_default" => {
                        c.mcp_prior_summaries_default =
                            v.parse().unwrap_or(c.mcp_prior_summaries_default)
                    }
                    "mcp_recent_calls_default" => {
                        c.mcp_recent_calls_default =
                            v.parse().unwrap_or(c.mcp_recent_calls_default)
                    }
                    "state_warn_calls" => {
                        c.state_warn_calls = v.parse().unwrap_or(c.state_warn_calls)
                    }
                    "sig_mode_enabled" => c.sig_mode_enabled = v == "true",
                    "sig_mode_threshold_lines" => {
                        c.sig_mode_threshold_lines =
                            v.parse().unwrap_or(c.sig_mode_threshold_lines)
                    }
                    "sig_mode_include_indented" => {
                        c.sig_mode_include_indented = v == "true"
                    }
                    "memory_file_warn_tokens" => {
                        c.memory_file_warn_tokens =
                            v.parse().unwrap_or(c.memory_file_warn_tokens)
                    }
                    "summary_format" => {
                        c.summary_format = match v {
                            "prose" | "structured" | "auto" => v.to_string(),
                            _ => "auto".to_string(),
                        };
                    }
                    "agent_prompt_max_tokens" => {
                        c.agent_prompt_max_tokens =
                            v.parse().unwrap_or(c.agent_prompt_max_tokens)
                    }
                    "plan_mode_passthrough" => c.plan_mode_passthrough = v == "true",
                    "nudge_enabled" => c.nudge_enabled = v == "true",
                    "nudge_error_threshold" => {
                        c.nudge_error_threshold =
                            v.parse().unwrap_or(c.nudge_error_threshold)
                    }
                    "nudge_file_mod_threshold" => {
                        c.nudge_file_mod_threshold =
                            v.parse().unwrap_or(c.nudge_file_mod_threshold)
                    }
                    "nudge_cmd_repeat_threshold" => {
                        c.nudge_cmd_repeat_threshold =
                            v.parse().unwrap_or(c.nudge_cmd_repeat_threshold)
                    }
                    "read_summarize_threshold_lines" => {
                        c.read_summarize_threshold_lines =
                            v.parse().unwrap_or(c.read_summarize_threshold_lines)
                    }
                    "summarize_threshold_bytes" => {
                        c.summarize_threshold_bytes =
                            v.parse().unwrap_or(c.summarize_threshold_bytes)
                    }
                    "screenshot_repeat_window_secs" => {
                        c.screenshot_repeat_window_secs =
                            v.parse().unwrap_or(c.screenshot_repeat_window_secs)
                    }
                    "strip_edit_preamble" => c.strip_edit_preamble = v == "true",
                    "handler_stats_enabled" => c.handler_stats_enabled = v == "true",
                    "context_window_tokens" => {
                        c.context_window_tokens = v.parse().unwrap_or(c.context_window_tokens)
                    }
                    "wrap_timeout_secs" => {
                        c.wrap_timeout_secs = v.parse().unwrap_or(c.wrap_timeout_secs)
                    }
                    "tokenizer_scale" => {
                        c.tokenizer_scale = v.parse().unwrap_or(c.tokenizer_scale)
                    }
                    "amplification_aware" => c.amplification_aware = v == "true",
                    "amplification_min_calls" => {
                        c.amplification_min_calls = v.parse().unwrap_or(c.amplification_min_calls)
                    }
                    "amplification_min_remaining" => {
                        c.amplification_min_remaining =
                            v.parse().unwrap_or(c.amplification_min_remaining)
                    }
                    "amplification_min_call_tokens" => {
                        c.amplification_min_call_tokens =
                            v.parse().unwrap_or(c.amplification_min_call_tokens)
                    }
                    "subagent_result_warn_tokens" => {
                        c.subagent_result_warn_tokens =
                            v.parse().unwrap_or(c.subagent_result_warn_tokens)
                    }
                    "quota_error_threshold" => {
                        c.quota_error_threshold =
                            v.parse().unwrap_or(c.quota_error_threshold)
                    }
                    "doctor_stale_days" => {
                        c.doctor_stale_days = v.parse().unwrap_or(c.doctor_stale_days)
                    }
                    "cache_idle_warn_secs" => {
                        c.cache_idle_warn_secs =
                            v.parse().unwrap_or(c.cache_idle_warn_secs)
                    }
                    "parallel_agent_burst_window_secs" => {
                        c.parallel_agent_burst_window_secs =
                            v.parse().unwrap_or(c.parallel_agent_burst_window_secs)
                    }
                    "parallel_agent_burst_threshold" => {
                        c.parallel_agent_burst_threshold =
                            v.parse().unwrap_or(c.parallel_agent_burst_threshold)
                    }
                    "subagent_total_warn_tokens" => {
                        c.subagent_total_warn_tokens =
                            v.parse().unwrap_or(c.subagent_total_warn_tokens)
                    }
                    "call_rate_warn_per_min" => {
                        c.call_rate_warn_per_min =
                            v.parse().unwrap_or(c.call_rate_warn_per_min)
                    }
                    "retrieve_enabled" => c.retrieve_enabled = v == "true",
                    "retrieve_ttl_days" => {
                        c.retrieve_ttl_days = v.parse().unwrap_or(c.retrieve_ttl_days)
                    }
                    "retrieve_min_lines" => {
                        c.retrieve_min_lines = v.parse().unwrap_or(c.retrieve_min_lines)
                    }
                    "log_template_enabled" => c.log_template_enabled = v == "true",
                    "log_template_min" => {
                        c.log_template_min = v.parse().unwrap_or(c.log_template_min)
                    }
                    "relevance_truncation_enabled" => {
                        c.relevance_truncation_enabled = v == "true"
                    }
                    "class_density" => c.class_density = v == "true",
                    "net_win_min_tokens" => {
                        c.net_win_min_tokens = v.parse().unwrap_or(c.net_win_min_tokens)
                    }
                    "preservation_guard" => c.preservation_guard = v == "true",
                    "preservation_floor" => {
                        c.preservation_floor = v.parse().unwrap_or(c.preservation_floor)
                    }
                    "builtin_filters" => c.builtin_filters = v == "true",
                    "wrap_bash" => c.wrap_bash = v == "true",
                    "bash_risk_patterns" => {
                        c.bash_risk_patterns =
                            v.split(',').map(|s| s.trim().to_string()).collect()
                    }
                    "success_collapse" => c.success_collapse = v == "true",
                    "success_collapse_deny" => {
                        c.success_collapse_deny =
                            v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
                    }
                    "flag_force" => {
                        c.flag_force = match v.to_lowercase().as_str() {
                            "off" => "off".to_string(),
                            "full" => "full".to_string(),
                            _ => "env".to_string(),
                        }
                    }
                    "flag_force_deny" => {
                        c.flag_force_deny =
                            v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
                    }
                    "buddy" => c.buddy = v == "true",
                    _ => {}
                }
            }
        }
        c
    }

    pub fn load() -> Self {
        let base = std::env::var("SQUEEZ_DIR").unwrap_or_else(|_| {
            format!("{}/.claude/squeez", crate::session::home_dir())
        });
        let path = format!("{}/config.ini", base);
        std::fs::read_to_string(&path)
            .map(|s| Self::from_str(&s))
            .unwrap_or_default()
    }

    /// Checks the bypass list against every `&&`/`;`-separated segment of
    /// `cmd`, not just the whole string — so `cd /path && npx vitest run`
    /// bypasses when `npx` is listed, even though the leading token is `cd` (#181).
    pub fn is_bypassed(&self, cmd: &str) -> bool {
        cmd.split("&&").flat_map(|s| s.split(';')).any(|segment| {
            let segment = segment.trim();
            self.bypass.iter().any(|b| segment.starts_with(b.as_str()))
        })
    }

    /// True if `cmd` matches a risk pattern — too dangerous to wrap, so it must
    /// run unwrapped under the host's native permission flow (#150).
    pub fn is_risky(&self, cmd: &str) -> bool {
        self.bash_risk_patterns
            .iter()
            .any(|p| !p.is_empty() && cmd.contains(p.as_str()))
    }

    /// Whether the PreToolUse hook should rewrite `cmd` to `squeez wrap '…'`.
    /// False when squeez is off, bash-wrap is disabled, the command is bypassed,
    /// or it's risky — in all those cases the original command runs and the
    /// host's native deny/ask rules apply to it unchanged.
    pub fn should_wrap_bash(&self, cmd: &str) -> bool {
        self.enabled && self.wrap_bash && !self.is_bypassed(cmd) && !self.is_risky(cmd)
    }

    /// Compression status for session banners. Derived from the real config so
    /// a disabled pipeline can never report itself as ON — `enabled=false` sat
    /// unnoticed for days behind a hardcoded "Compression: ON" banner.
    pub fn compression_status_label(&self) -> String {
        if !self.enabled {
            "OFF (enabled=false — run `squeez config set enabled true`)".to_string()
        } else if !self.wrap_bash {
            "PARTIAL (wrap_bash=false)".to_string()
        } else {
            "ON".to_string()
        }
    }
}

#[cfg(test)]
mod wrap_safety_tests {
    use super::*;

    #[test]
    fn compression_status_reflects_config() {
        let mut c = Config::default();
        assert_eq!(c.compression_status_label(), "ON");
        c.wrap_bash = false;
        assert_eq!(c.compression_status_label(), "PARTIAL (wrap_bash=false)");
        c.enabled = false;
        assert!(c.compression_status_label().starts_with("OFF (enabled=false"));
        c.wrap_bash = true;
        assert!(c.compression_status_label().starts_with("OFF (enabled=false"));
    }

    #[test]
    fn risky_commands_are_not_wrapped() {
        let c = Config::default();
        for cmd in [
            "rm -rf /tmp/x",
            "git push --force origin main",
            "git push -f",
            "npm publish",
            "git reset --hard HEAD~3",
        ] {
            assert!(c.is_risky(cmd), "{cmd} should be risky");
            assert!(!c.should_wrap_bash(cmd), "{cmd} must not be wrapped");
        }
    }

    #[test]
    fn ordinary_commands_are_wrapped() {
        let c = Config::default();
        for cmd in ["ls -la", "git status", "cargo test", "grep -rn TODO src/"] {
            assert!(!c.is_risky(cmd));
            assert!(c.should_wrap_bash(cmd), "{cmd} should be wrapped");
        }
    }

    #[test]
    fn disabling_wrap_bash_stops_all_wrapping() {
        let mut c = Config::default();
        c.wrap_bash = false;
        assert!(!c.should_wrap_bash("ls -la"));
    }

    #[test]
    fn bypassed_commands_are_not_wrapped() {
        let c = Config::default(); // bypass includes ssh, psql, …
        assert!(!c.should_wrap_bash("ssh host uptime"));
    }

    #[test]
    fn bypass_matches_segment_after_cd_and(){
        let mut c = Config::default();
        c.bypass.push("npx vitest".to_string());
        assert!(c.is_bypassed("cd /path/to/repo && npx vitest run"));
        assert!(!c.should_wrap_bash("cd /path/to/repo && npx vitest run"));
    }

    #[test]
    fn bypass_matches_segment_after_semicolon() {
        let mut c = Config::default();
        c.bypass.push("mysql".to_string());
        assert!(c.is_bypassed("cd /tmp; mysql -u root"));
    }

    #[test]
    fn bypass_does_not_match_unrelated_compound_command() {
        let c = Config::default();
        assert!(!c.is_bypassed("cd /path/to/repo && npx vitest run"));
    }
}
