use crate::commands::persona::Persona;

#[derive(Debug, Clone)]
pub struct Config {
    pub enabled: bool,
    pub show_header: bool,
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
    pub summarize_threshold_lines: usize,
    // ── Output / memory-file flags ──────────────────────────────────────
    pub persona: Persona,
    pub auto_compress_md: bool,
    pub lang: String,
    // ── Token economy (phase 7) ───────────────────────────────────────────
    /// Fraction of budget at which sub-agent cost triggers a warning (default 0.50).
    pub agent_warn_threshold_pct: f32,
    /// Predict pressure warning when calls remaining < this (default 20).
    pub burn_rate_warn_calls: u64,
    /// Estimated tokens per sub-agent spawn (default 200_000).
    pub agent_spawn_cost: u64,
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            show_header: true,
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
            compact_threshold_tokens: 120_000,
            memory_retention_days: 30,
            adaptive_intensity: true,
            context_cache_enabled: true,
            redundancy_cache_enabled: true,
            summarize_threshold_lines: 300,
            persona: Persona::Ultra,
            auto_compress_md: true,
            lang: "en".to_string(),
            agent_warn_threshold_pct: 0.50,
            burn_rate_warn_calls: 20,
            agent_spawn_cost: 200_000,
            read_max_lines: 300,
            grep_max_results: 100,
            max_call_log: 32,
            recent_window: 16,
            similarity_threshold: 0.85,
            ultra_trigger_pct: 0.80,
            mcp_prior_summaries_default: 5,
            mcp_recent_calls_default: 10,
        }
    }
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
                    "show_header" => c.show_header = v == "true",
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
                    "summarize_threshold_lines" => {
                        c.summarize_threshold_lines =
                            v.parse().unwrap_or(c.summarize_threshold_lines)
                    }
                    "persona" => c.persona = crate::commands::persona::from_str(v),
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

    pub fn is_bypassed(&self, cmd: &str) -> bool {
        self.bypass.iter().any(|b| cmd.starts_with(b.as_str()))
    }
}
