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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            show_header: true,
            max_lines: 200,
            dedup_min: 3,
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
            compact_threshold_tokens: 160_000,
            memory_retention_days: 30,
            adaptive_intensity: true,
            context_cache_enabled: true,
            redundancy_cache_enabled: true,
            summarize_threshold_lines: 500,
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
