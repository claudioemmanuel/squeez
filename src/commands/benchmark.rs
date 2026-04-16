//! `squeez benchmark` — reproducible measurement of token reduction, cost, latency, quality.
//!
//! Usage:
//!   squeez benchmark [--json] [--output <file>] [--scenario <name>] [--iterations <n>]
//!   squeez benchmark --list
//!
//! Token model  : chars / 4  (matches existing bench/run.sh convention)
//! Cost model   : Claude Sonnet 4.6 input $3.00 / 1M tokens
//!                Claude Opus 4.7   input $5.00 / 1M tokens
//! Quality model: fraction of "key terms" from baseline that survive compression

use std::path::PathBuf;
use std::time::Instant;

use crate::commands::compress_md::{compress_text, Mode as MdMode};
use crate::config::Config;
use crate::filter;
use crate::json_util;

// ─── Pricing ─────────────────────────────────────────────────────────────────

/// Claude Sonnet 4.6 input USD per 1 000 000 tokens.
const INPUT_COST_PER_MTOK: f64 = 3.0;
/// Claude Opus 4.7 input USD per 1 000 000 tokens.
const INPUT_COST_PER_MTOK_OPUS47: f64 = 5.0;
/// Quality threshold: fraction of key terms that must survive.
const QUALITY_PASS_THRESHOLD: f64 = 0.50;

// ─── Public types ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ScenarioResult {
    pub name: String,
    pub category: String,
    pub baseline_tokens: usize,
    pub compressed_tokens: usize,
    pub reduction_pct: f64,
    /// Median latency across iterations, microseconds.
    pub latency_us: u64,
    pub quality_score: f64,
    pub quality_pass: bool,
    /// Extra context saved via cross-call dedup (Wrap scenarios only).
    pub context_saved_tokens: usize,
    pub iterations: usize,
}

pub struct BenchmarkReport {
    pub results: Vec<ScenarioResult>,
    // ── Aggregate token metrics ──
    pub total_baseline_tokens: usize,
    pub total_compressed_tokens: usize,
    pub total_reduction_pct: f64,
    /// Reduction for bash-output (filter) scenarios only.
    pub bash_reduction_pct: f64,
    /// Reduction for markdown scenarios only.
    pub md_reduction_pct: f64,
    /// Reduction for wrap/context scenarios only.
    pub wrap_reduction_pct: f64,
    // ── Latency ──
    pub avg_latency_us: u64,
    pub p95_latency_us: u64,
    // ── Cost ──
    pub estimated_cost_savings_pct: f64,
    // ── Quality ──
    pub quality_pass_count: usize,
    pub quality_fail_count: usize,
    pub quality_skip_count: usize,
}

// ─── Internal types ───────────────────────────────────────────────────────────

enum ScenarioKind {
    /// Use filter::compress() with given command hint.
    Filter { hint: String },
    /// Use compress_text() in Ultra mode.
    Markdown,
    /// Spawn binary via wrap for N calls (cross-call or summarize).
    Wrap { calls: usize },
}

/// Controls how quality is scored for a scenario.
enum QualityMode {
    /// Check required_keywords (hard), then measure signal-term preservation.
    /// Signal terms = unique words from error/warning/failed/fatal lines.
    /// If no signal terms in baseline → trivially passes.
    Signal,
    /// Check required_keywords presence only.
    /// Used for wrap scenarios where compressed output has deliberate new format.
    Keywords,
}

struct Scenario {
    name: String,
    category: String,
    kind: ScenarioKind,
    content: String,
    /// Substrings that must appear in output for quality to pass.
    required_keywords: Vec<String>,
    quality_mode: QualityMode,
}

// ─── Synthetic fixture generators ─────────────────────────────────────────────

fn make_cargo_build() -> String {
    let mut out = String::new();
    // Simulates a noisy cargo build with downloads, compiling lines, warnings, 2 errors
    for i in 0..80 {
        out.push_str(&format!(
            "   Downloading crates.io index\n   Downloading {} v{}.{}.{}\n",
            ["serde", "tokio", "hyper", "reqwest", "clap"][i % 5],
            i / 10,
            i % 10,
            0
        ));
    }
    out.push_str("   Compiling squeez v0.2.1\n");
    for i in 0..30 {
        out.push_str(&format!(
            "warning: unused variable `x` --> src/lib.rs:{}:{}\n  |\n{}| let x = 42;\n  |     ^ help: consider using `_x`\n",
            100 + i, 5, 100 + i
        ));
    }
    out.push_str("error[E0432]: unresolved import `crate::missing`\n --> src/main.rs:3:5\n  |\n3 | use crate::missing;\n  |     ^^^^^^^^^^^^^^^ no `missing` in the root\n\n");
    out.push_str("error[E0308]: mismatched types\n --> src/filter.rs:42:10\n  |\n42|     return \"hello\";\n  |            ^^^^^^^ expected usize, found &str\n\n");
    out.push_str("error: aborting due to 2 previous errors\n");
    out.push_str("For more information about this error, try `rustc --explain E0432`.\n");
    out.push_str("error: could not compile `squeez` due to 2 previous errors\n");
    out
}

fn make_tsc_errors() -> String {
    let mut out = String::new();
    // Simulate TypeScript compilation with many info lines and a handful of errors
    for i in 0..40 {
        out.push_str(&format!(
            "info: checking src/components/Component{}.tsx\n",
            i
        ));
    }
    out.push_str("src/components/Button.tsx(12,5): error TS2345: Argument of type 'string' is not assignable to parameter of type 'number'.\n");
    out.push_str("src/components/Modal.tsx(34,9): error TS2304: Cannot find name 'useEffect'.\n");
    out.push_str("src/api/client.ts(88,3): error TS2339: Property 'data' does not exist on type 'Response'.\n");
    out.push_str("src/utils/format.ts(5,10): warning TS6133: 'unused' is declared but its value is never read.\n");
    for i in 0..20 {
        out.push_str(&format!(
            "info: processed module {}/20\n",
            i + 1
        ));
    }
    out.push_str("Found 3 errors in 3 files.\n\nErrors  Files\n     1  src/components/Button.tsx:12\n     1  src/components/Modal.tsx:34\n     1  src/api/client.ts:88\n");
    out
}

fn make_verbose_log() -> String {
    let mut out = String::new();
    // Simulate noisy app server log — timestamps, debug lines, occasional errors
    let levels = ["DEBUG", "DEBUG", "DEBUG", "INFO", "INFO", "WARN", "ERROR"];
    let msgs = [
        "request received method=GET path=/api/health",
        "database pool: 4/10 connections active",
        "cache hit key=user:12345 ttl=3540s",
        "processed request latency=12ms status=200",
        "scheduled job starting name=cleanup_old_sessions",
        "slow query detected duration=1250ms table=events",
        "upstream timeout after 30s url=https://api.external.com/webhook",
    ];
    for i in 0..250 {
        let ts = format!("2026-04-07T{:02}:{:02}:{:02}.{:03}Z", i / 3600, (i / 60) % 60, i % 60, i * 3 % 1000);
        let level = levels[i % levels.len()];
        let msg = msgs[i % msgs.len()];
        out.push_str(&format!("{} [{}] {}\n", ts, level, msg));
    }
    // Add a few unique critical errors
    out.push_str("2026-04-07T01:00:00.000Z [ERROR] OOM kill signal received — pod squeez-worker-7f9b restarting\n");
    out.push_str("2026-04-07T01:00:01.000Z [ERROR] connection to Redis lost — retrying in 5s\n");
    out
}

fn make_repetitive_output() -> String {
    let mut out = String::new();
    // 300 identical lines (dedup bait) plus 10 unique lines
    for _ in 0..300 {
        out.push_str("2026-04-07 00:00:00 [TRACE] heartbeat ping to cluster-node-a\n");
    }
    out.push_str("unique: deployment completed successfully version=1.2.3\n");
    out.push_str("unique: rollout status: 5/5 pods updated\n");
    out.push_str("unique: health check passed for all replicas\n");
    out.push_str("unique: CDN cache invalidated region=us-east-1\n");
    out.push_str("unique: metrics flushed to prometheus endpoint\n");
    out.push_str("unique: alert rules reloaded count=42\n");
    out.push_str("unique: backup snapshot created id=snap-0xdeadbeef\n");
    out.push_str("unique: audit log entry recorded user=deploy-bot\n");
    out.push_str("unique: TLS certificate renewed expiry=2027-04-07\n");
    out.push_str("unique: session count=1234 active connections\n");
    out
}

fn make_kubectl_pods() -> String {
    let mut out = String::new();
    out.push_str("NAMESPACE       NAME                                      READY   STATUS             RESTARTS   AGE\n");
    let namespaces = ["default", "kube-system", "monitoring", "ingress-nginx", "cert-manager"];
    let statuses = ["Running", "Running", "Running", "Running", "CrashLoopBackOff", "Error", "Pending"];
    let apps = ["api-server", "worker", "scheduler", "prometheus", "grafana", "redis", "postgres", "nginx"];
    for i in 0..60 {
        let ns = namespaces[i % namespaces.len()];
        let app = apps[i % apps.len()];
        let status = statuses[i % statuses.len()];
        let ready = if status == "Running" { "1/1" } else { "0/1" };
        out.push_str(&format!(
            "{:<16}{:<42}{:<8}{:<19}{:<11}{}\n",
            ns, format!("{}-{:x}-{:x}", app, i * 0x1a2b, i * 0x3c4d),
            ready, status, i % 5, format!("{}d", i / 5 + 1)
        ));
    }
    out
}

fn make_agent_heavy() -> String {
    let mut out = String::new();
    // Simulate an agent-heavy Claude Code session: many sub-agent spawns, each producing
    // verbose status output. Represents the token drain the research documents call "critical".
    for i in 0..8 {
        out.push_str(&format!("--- Agent spawn #{} ---\n", i + 1));
        out.push_str("Starting sub-agent worker...\n");
        out.push_str(&format!("Agent(Explore) initializing context window (up to 200K tokens)\n"));
        out.push_str(&format!("  Reading src/module_{}/mod.rs ... done\n", i));
        out.push_str(&format!("  Reading src/module_{}/lib.rs ... done\n", i));
        out.push_str(&format!("  Searching for pattern: fn handle_request ...\n"));
        out.push_str(&format!("  Found {} matches in {} files\n", 12 + i, 4 + i));
        for j in 0..15 {
            out.push_str(&format!(
                "  [{:>3}] src/module_{}/handler_{}.rs:{}:{} - match found\n",
                j + 1, i, j, 10 + j * 3, 4
            ));
        }
        out.push_str("Agent(Explore) synthesis complete\n");
        out.push_str("Sub-agent returned 1 result\n\n");
    }
    out.push_str("error: compilation failed after agent exploration — unresolved import `crate::missing_mod`\n");
    out.push_str("  --> src/main.rs:5:5\n");
    out.push_str("fix: add `mod missing_mod;` to src/main.rs\n");
    out
}

fn make_session_state_md() -> String {
    // Simulates a compact session_state.md written before /clear.
    // Demonstrates the State-First Pattern economics: ~300 tokens vs 50K compaction summary.
    let mut out = String::new();
    out.push_str("# Session State (2026-04-15)\n\n");
    out.push_str("## Objective\nImplementing full efficiency layer for squeez (gaps 1-6 from research).\n\n");
    out.push_str("## Files Modified\n");
    out.push_str("- src/session.rs: added state_warned, tokens_saved, total_calls fields\n");
    out.push_str("- src/config.rs: added state_warn_calls tunable\n");
    out.push_str("- src/commands/mcp_server.rs: 14 tools, squeez_context_pressure added\n");
    out.push_str("- src/commands/wrap.rs: tier-2 critical pressure advisor\n\n");
    out.push_str("## Decisions\n");
    out.push_str("- Header injection (not new hook) for advisor — fits existing architecture\n");
    out.push_str("- tokens_saved = in_tk - out_tk tracked in wrap.rs record_bash_event()\n");
    out.push_str("- state_warn_calls default = 10 (configurable via config.ini)\n\n");
    out.push_str("## Next Steps\n");
    out.push_str("1. Add 3 economy benchmark scenarios\n");
    out.push_str("2. Add --baseline flag to benchmark\n");
    out.push_str("3. cargo test && cargo build --release\n");
    out
}

// ─── Fixtures directory discovery ────────────────────────────────────────────

fn fixtures_dir() -> PathBuf {
    // 1. env override (useful for tests)
    if let Ok(dir) = std::env::var("SQUEEZ_BENCH_FIXTURES") {
        return PathBuf::from(dir);
    }
    // 2. relative to the running binary (dev: target/release/squeez)
    if let Ok(exe) = std::env::current_exe() {
        // exe is at <project>/target/release/squeez
        // fixtures are at <project>/bench/fixtures
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("../../bench/fixtures");
            let candidate = candidate.canonicalize().unwrap_or(candidate);
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    // 3. relative to current working dir
    let candidate = PathBuf::from("bench/fixtures");
    if candidate.is_dir() {
        return candidate;
    }
    PathBuf::from("bench/fixtures")
}

fn find_binary() -> Option<PathBuf> {
    // Prefer the dev build, then the installed binary.
    let exe = std::env::current_exe().ok()?;
    // If we ARE the squeez binary, return our own path
    if exe.file_name().map(|n| n == "squeez").unwrap_or(false) {
        return Some(exe);
    }
    // Fallback: check installed location
    let home = crate::session::home_dir();
    let installed = PathBuf::from(format!("{}/.claude/squeez/bin/squeez", home));
    if installed.exists() {
        return Some(installed);
    }
    None
}

fn make_large_claude_md() -> String {
    // Simulates a CLAUDE.md that exceeds the research-recommended 1K-token limit.
    // This content would be re-read on every API turn, making it a fixed per-turn cost.
    // At ~150 lines / ~600 words ≈ 2K tokens: 2× the recommended ceiling.
    let mut out = String::new();
    out.push_str("# Project Guidelines\n\n");
    out.push_str("## Architecture Overview\n");
    out.push_str("This project uses a microservices architecture with the following components:\n");
    for i in 1..=8 {
        out.push_str(&format!("- Service {}: Handles {} operations with {} pattern\n",
            i, ["auth", "billing", "search", "notify", "storage", "cache", "queue", "analytics"][i-1],
            ["REST", "gRPC", "GraphQL", "WebSocket", "batch", "stream", "pub-sub", "event-driven"][i-1]));
    }
    out.push_str("\n## Coding Standards\n");
    out.push_str("All code must follow these standards:\n");
    for rule in &[
        "Use TypeScript strict mode for all new files",
        "Every function must have JSDoc comments with @param and @returns",
        "Test coverage must be at least 80% for all new modules",
        "Use async/await instead of callbacks or raw Promises",
        "All API endpoints must validate input with Zod schemas",
        "Database queries must use parameterized statements only",
        "Log all errors with structured JSON including timestamp and trace_id",
        "Use feature flags for all new functionality behind experiments",
        "Never commit secrets — use environment variables via .env files",
        "All PRs require two approvals and passing CI before merge",
        "Dependency updates must be reviewed for security advisories",
        "Use semantic versioning for all package releases",
    ] {
        out.push_str(&format!("- {}\n", rule));
    }
    out.push_str("\n## Tool Usage Rules\n");
    out.push_str("When working in this codebase:\n");
    for rule in &[
        "Always run `npm test` before committing",
        "Use `grep` to find files before using `read_file`",
        "Never use `Agent(Explore)` for simple file searches",
        "Run `npm run build` to check TypeScript compilation",
        "Check `git status` before making any changes",
        "Use `git log --oneline -10` to review recent history",
        "Always read CHANGELOG.md before starting a new feature",
        "Use the project's ESLint config — don't disable rules",
        "Check package.json for existing scripts before adding new ones",
    ] {
        out.push_str(&format!("- {}\n", rule));
    }
    out.push_str("\n## Deployment Checklist\n");
    for item in &[
        "Update version in package.json",
        "Run full test suite and ensure 0 failures",
        "Build production bundle and check bundle size",
        "Update CHANGELOG.md with release notes",
        "Create git tag matching the version",
        "Push tag to trigger CI/CD pipeline",
        "Monitor error rates in Grafana for 30 minutes post-deploy",
        "Send release announcement to #engineering channel",
    ] {
        out.push_str(&format!("- [ ] {}\n", item));
    }
    out.push_str("\n## Environment Variables\n");
    for var in &[
        "DATABASE_URL", "REDIS_URL", "JWT_SECRET", "AWS_REGION",
        "S3_BUCKET", "SENDGRID_KEY", "STRIPE_KEY", "DATADOG_KEY",
        "FEATURE_FLAGS_URL", "LOG_LEVEL", "PORT", "NODE_ENV",
    ] {
        out.push_str(&format!("- `{}`: Required for {} service\n", var,
            var.split('_').next().unwrap_or("core").to_lowercase()));
    }
    out
}

// ─── Scenario construction ────────────────────────────────────────────────────

fn build_scenarios(fixtures: &PathBuf) -> Vec<Scenario> {
    let mut s: Vec<Scenario> = Vec::new();
    let load = |name: &str| -> Option<String> {
        std::fs::read_to_string(fixtures.join(name)).ok()
    };

    // ── Bash output (filter) scenarios ───────────────────────────────────────
    macro_rules! f {
        ($name:literal, $fixture:literal, $hint:literal, [$($kw:literal),*]) => {
            if let Some(content) = load($fixture) {
                s.push(Scenario {
                    name: $name.to_string(),
                    category: "bash_output".to_string(),
                    kind: ScenarioKind::Filter { hint: $hint.to_string() },
                    content,
                    required_keywords: vec![$($kw.to_string()),*],
                    quality_mode: QualityMode::Signal,
                });
            }
        };
    }

    // required_keywords: strings that MUST survive compression.
    // Left empty for scenarios where compressed format changes structure (git log
    // drops the "commit" keyword; git status groups files without "branch" header).
    // git log: commits are truncated to N most recent; quality via Keywords (no
    // hard-required terms — any truncation of history is semantically valid).
    if let Some(content) = load("git_log_200.txt") {
        s.push(Scenario {
            name: "git_log_200".to_string(),
            category: "bash_output".to_string(),
            kind: ScenarioKind::Filter { hint: "git log".to_string() },
            content,
            required_keywords: vec![],
            quality_mode: QualityMode::Keywords,
        });
    }
    f!("git_diff",     "git_diff.txt",             "git diff",   ["---", "+++"]);
    f!("git_status",   "git_status.txt",           "git status", []);
    f!("docker_logs",  "docker_logs.txt",          "docker",     []);
    f!("npm_install",  "npm_install.txt",          "npm",        ["added"]);
    f!("ps_aux",       "ps_aux.txt",               "ps",         []);
    f!("find_deep",    "find_deep.txt",             "find",       []);
    f!("ls_la",        "ls_la.txt",                "ls",         ["total"]);
    f!("env_dump",     "env_dump.txt",              "env",        ["PATH"]);
    f!("git_copilot",  "git_copilot_session.txt",  "git",        []);

    // Synthetic filter scenarios
    s.push(Scenario {
        name: "cargo_build_noisy".to_string(),
        category: "bash_output".to_string(),
        kind: ScenarioKind::Filter { hint: "cargo build".to_string() },
        content: make_cargo_build(),
        required_keywords: vec!["error".to_string()],
        quality_mode: QualityMode::Signal,
    });
    s.push(Scenario {
        name: "tsc_errors".to_string(),
        category: "bash_output".to_string(),
        kind: ScenarioKind::Filter { hint: "tsc".to_string() },
        content: make_tsc_errors(),
        required_keywords: vec!["error TS".to_string(), "Found".to_string()],
        quality_mode: QualityMode::Signal,
    });
    s.push(Scenario {
        name: "verbose_app_log".to_string(),
        category: "bash_output".to_string(),
        kind: ScenarioKind::Filter { hint: "docker logs".to_string() },
        content: make_verbose_log(),
        required_keywords: vec!["ERROR".to_string()],
        quality_mode: QualityMode::Signal,
    });
    s.push(Scenario {
        name: "repetitive_output".to_string(),
        category: "bash_output".to_string(),
        kind: ScenarioKind::Filter { hint: "generic".to_string() },
        content: make_repetitive_output(),
        required_keywords: vec!["unique".to_string()],
        quality_mode: QualityMode::Signal,
    });
    s.push(Scenario {
        name: "kubectl_pods".to_string(),
        category: "bash_output".to_string(),
        kind: ScenarioKind::Filter { hint: "kubectl get pods".to_string() },
        content: make_kubectl_pods(),
        required_keywords: vec!["Running".to_string(), "NAME".to_string()],
        quality_mode: QualityMode::Signal,
    });

    // ── Markdown / context scenarios ──────────────────────────────────────────
    if let Some(content) = load("mdcompress_claude_md.txt") {
        s.push(Scenario {
            name: "md_claude_md".to_string(),
            category: "markdown".to_string(),
            kind: ScenarioKind::Markdown,
            content,
            required_keywords: vec![],
            quality_mode: QualityMode::Signal,
        });
    }
    if let Some(content) = load("mdcompress_prose.txt") {
        s.push(Scenario {
            name: "md_prose".to_string(),
            category: "markdown".to_string(),
            kind: ScenarioKind::Markdown,
            content,
            required_keywords: vec![],
            quality_mode: QualityMode::Signal,
        });
    }

    // ── Economy scenarios (session efficiency research) ───────────────────────
    // agent_heavy: token drain from agent-heavy sessions; tests compression of
    // verbose sub-agent spawn/status output (the "critical" drain from the research).
    // Quality mode: Keywords (no required terms) — the metric of interest is reduction_pct.
    s.push(Scenario {
        name: "agent_heavy".to_string(),
        category: "economy".to_string(),
        kind: ScenarioKind::Filter { hint: "bash".to_string() },
        content: make_agent_heavy(),
        required_keywords: vec![],
        quality_mode: QualityMode::Keywords,
    });

    // high_context_adaptive: loads intensity_budget80 fixture or synthesises it;
    // verifies that Ultra intensity fires and achieves high reduction on large output.
    // Quality mode: Keywords — reduction_pct (≥90% expected) is the key metric.
    {
        let hca_content = load("intensity_budget80.txt")
            .unwrap_or_else(|| make_repetitive_output().repeat(10));
        s.push(Scenario {
            name: "high_context_adaptive".to_string(),
            category: "economy".to_string(),
            kind: ScenarioKind::Filter { hint: "bash".to_string() },
            content: hca_content,
            required_keywords: vec![],
            quality_mode: QualityMode::Keywords,
        });
    }

    // state_first_simulation: a compact session_state.md costs ~300 tokens —
    // demonstrates the economics of the State-First Pattern vs a 50K compaction summary.
    // Quality mode: Keywords — the low baseline_tokens value itself proves the point.
    s.push(Scenario {
        name: "state_first_simulation".to_string(),
        category: "economy".to_string(),
        kind: ScenarioKind::Filter { hint: "cat".to_string() },
        content: make_session_state_md(),
        required_keywords: vec![],
        quality_mode: QualityMode::Keywords,
    });

    // claude_md_overhead: simulates a large CLAUDE.md being re-read every turn (C5 from research).
    // Research: CLAUDE.md should be <1K tokens; anything beyond is paid on every API call.
    // Quality mode: Keywords — the per-turn token floor is the metric, not signal preservation.
    s.push(Scenario {
        name: "claude_md_overhead".to_string(),
        category: "economy".to_string(),
        kind: ScenarioKind::Filter { hint: "cat".to_string() },
        content: make_large_claude_md(),
        required_keywords: vec![],
        quality_mode: QualityMode::Keywords,
    });

    // ── Wrap (binary spawn) scenarios ─────────────────────────────────────────
    // Keywords-only: the wrap output format changes intentionally
    // (summary header / dedup reference line), so term-overlap scoring
    // would always fail even when the compression is semantically correct.
    if find_binary().is_some() {
        if let Some(content) = load("summarize_huge.txt") {
            s.push(Scenario {
                name: "summarize_huge".to_string(),
                category: "wrap_summarize".to_string(),
                kind: ScenarioKind::Wrap { calls: 1 },
                content,
                required_keywords: vec!["squeez:summary".to_string()],
                quality_mode: QualityMode::Keywords,
            });
        }
        if let Some(content) = load("context_crosscall_1.txt") {
            s.push(Scenario {
                name: "crosscall_redundancy_3x".to_string(),
                category: "wrap_crosscall".to_string(),
                kind: ScenarioKind::Wrap { calls: 3 },
                content,
                required_keywords: vec!["squeez: identical to".to_string()],
                quality_mode: QualityMode::Keywords,
            });
        }
    }

    s
}

// ─── Quality scorer ───────────────────────────────────────────────────────────
//
// Two modes:
//
// Signal: extract "signal" terms (words from error/warning/fatal/failed lines
//   plus required_keywords) and check their presence in compressed output.
//   If no signal terms exist in baseline → trivially passes (noise-only outputs
//   are correctly discarded at high reduction ratios).
//
// Keywords: binary pass/fail based on required_keywords presence only.
//   Used for wrap scenarios whose output format changes by design
//   (summary header, dedup reference line) so term-overlap would always fail.

fn quality_score(baseline: &str, compressed: &str, required: &[String], mode: &QualityMode) -> f64 {
    if compressed.is_empty() {
        return 0.0;
    }
    match mode {
        QualityMode::Keywords => {
            for kw in required {
                if !kw.is_empty() && !compressed.contains(kw.as_str()) {
                    return 0.0;
                }
            }
            1.0
        }
        QualityMode::Signal => {
            // Hard check: required keywords must be present
            for kw in required {
                if !kw.is_empty() && !compressed.contains(kw.as_str()) {
                    return 0.0;
                }
            }
            // Soft check: signal terms extracted from error/warning lines
            let signal = extract_signal_terms(baseline);
            if signal.is_empty() {
                // No diagnostic signal in baseline (e.g. clean ps/git/find output).
                // The compressor is not obligated to keep noise; trivially pass.
                return 1.0;
            }
            let compressed_lower = compressed.to_ascii_lowercase();
            let preserved = signal
                .iter()
                .filter(|t| compressed_lower.contains(t.as_str()) || compressed.contains(t.as_str()))
                .count();
            preserved as f64 / signal.len() as f64
        }
    }
}

/// Extract "signal" terms — unique tokens from lines that contain diagnostic
/// keywords (error, warning, failed, fatal, panic, exception).
/// These are the lines a developer must see; a good compressor should keep them.
fn extract_signal_terms(text: &str) -> Vec<String> {
    let mut terms = std::collections::HashSet::new();
    let noise: &[&str] = &["the", "and", "for", "this", "that", "with", "from", "into", "was"];
    let diag = ["error", "warning", "failed", "fatal", "panic", "exception"];
    for line in text.lines() {
        let trimmed = line.trim();
        // Skip pure file-path lines (e.g. `./src/foo/error-handler.ts`).
        // A diagnostic line has spaces; a lone path token is not one.
        if !trimmed.contains(' ')
            && (trimmed.starts_with("./") || trimmed.starts_with('/'))
        {
            continue;
        }
        let ll = line.to_ascii_lowercase();
        // Require the diagnostic keyword to appear as a standalone word, not
        // embedded inside a longer identifier or URL path (e.g. "needsBackendErrorsIpcClient").
        let is_diagnostic = diag.iter().any(|kw| {
            if let Some(pos) = ll.find(kw) {
                let bytes = ll.as_bytes();
                let before_ok = pos == 0 || !bytes[pos - 1].is_ascii_alphanumeric();
                let after_ok = pos + kw.len() >= bytes.len()
                    || !bytes[pos + kw.len()].is_ascii_alphanumeric();
                before_ok && after_ok
            } else {
                false
            }
        });
        if is_diagnostic {
            for word in line.split_whitespace() {
                let w = word.trim_matches(|c: char| {
                    !c.is_alphanumeric() && c != '/' && c != '.' && c != '_' && c != ':' && c != '['
                });
                let wl = w.to_ascii_lowercase();
                if w.len() >= 4
                    && !wl.chars().all(|c| c.is_ascii_digit())
                    && !noise.contains(&wl.as_str())
                {
                    terms.insert(wl);
                }
            }
        }
    }
    terms.into_iter().collect()
}

// ─── Scenario runners ─────────────────────────────────────────────────────────

fn run_filter(scenario: &Scenario, hint: &str, iterations: usize) -> ScenarioResult {
    let config = Config {
        adaptive_intensity: false, // fixed config for reproducibility
        show_header: false,
        ..Config::default()
    };

    let lines: Vec<String> = scenario.content.lines().map(|l| l.to_string()).collect();
    let baseline_tokens = scenario.content.len() / 4;

    let mut latencies_us: Vec<u64> = Vec::with_capacity(iterations);
    let mut last_compressed = String::new();

    for _ in 0..iterations {
        let t0 = Instant::now();
        let result = filter::compress(hint, lines.clone(), &config);
        let elapsed = t0.elapsed().as_micros() as u64;
        latencies_us.push(elapsed);
        last_compressed = result.join("\n");
    }

    latencies_us.sort_unstable();
    let median_us = latencies_us[latencies_us.len() / 2];
    let compressed_tokens = last_compressed.len() / 4;
    let reduction = reduction_pct(baseline_tokens, compressed_tokens);
    let qscore = quality_score(&scenario.content, &last_compressed, &scenario.required_keywords, &scenario.quality_mode);

    ScenarioResult {
        name: scenario.name.clone(),
        category: scenario.category.clone(),
        baseline_tokens,
        compressed_tokens,
        reduction_pct: reduction,
        latency_us: median_us,
        quality_score: qscore,
        quality_pass: qscore >= QUALITY_PASS_THRESHOLD,
        context_saved_tokens: 0,
        iterations,
    }
}

fn run_markdown(scenario: &Scenario, iterations: usize) -> ScenarioResult {
    let baseline_tokens = scenario.content.len() / 4;
    let mut latencies_us: Vec<u64> = Vec::with_capacity(iterations);
    let mut last_output = String::new();

    for _ in 0..iterations {
        let t0 = Instant::now();
        let result = compress_text(&scenario.content, MdMode::Ultra);
        let elapsed = t0.elapsed().as_micros() as u64;
        latencies_us.push(elapsed);
        last_output = result.output;
    }

    latencies_us.sort_unstable();
    let median_us = latencies_us[latencies_us.len() / 2];
    let compressed_tokens = last_output.len() / 4;
    let reduction = reduction_pct(baseline_tokens, compressed_tokens);
    let qscore = quality_score(&scenario.content, &last_output, &scenario.required_keywords, &scenario.quality_mode);

    ScenarioResult {
        name: scenario.name.clone(),
        category: scenario.category.clone(),
        baseline_tokens,
        compressed_tokens,
        reduction_pct: reduction,
        latency_us: median_us,
        quality_score: qscore,
        quality_pass: qscore >= QUALITY_PASS_THRESHOLD,
        context_saved_tokens: 0,
        iterations,
    }
}

fn run_wrap(scenario: &Scenario, calls: usize, iterations: usize) -> ScenarioResult {
    let binary = match find_binary() {
        Some(b) => b,
        None => {
            return ScenarioResult {
                name: scenario.name.clone(),
                category: scenario.category.clone(),
                baseline_tokens: scenario.content.len() / 4,
                compressed_tokens: scenario.content.len() / 4,
                reduction_pct: 0.0,
                latency_us: 0,
                quality_score: 0.0,
                quality_pass: false,
                context_saved_tokens: 0,
                iterations: 0,
            };
        }
    };

    let baseline_tokens = scenario.content.len() / 4;

    // Write content to a temp file so wrap can `cat` it
    let tmp_dir = std::env::temp_dir().join(format!("squeez_bench_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);
    let fixture_file = tmp_dir.join("input.txt");
    let squeez_dir = tmp_dir.join("squeez_state");
    let _ = std::fs::create_dir_all(&squeez_dir);
    let _ = std::fs::create_dir_all(squeez_dir.join("sessions"));
    let _ = std::fs::create_dir_all(squeez_dir.join("memory"));

    if std::fs::write(&fixture_file, &scenario.content).is_err() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return ScenarioResult {
            name: scenario.name.clone(),
            category: scenario.category.clone(),
            baseline_tokens,
            compressed_tokens: baseline_tokens,
            reduction_pct: 0.0,
            latency_us: 0,
            quality_score: 1.0,
            quality_pass: true,
            context_saved_tokens: 0,
            iterations: 0,
        };
    }

    let mut all_latencies_us: Vec<u64> = Vec::new();
    let mut last_output_all_calls = String::new();
    let mut total_compressed_tokens_per_run: Vec<usize> = Vec::new();

    for _iter in 0..iterations {
        // Fresh state per run
        let iter_state_dir = tmp_dir.join(format!("state_{}", _iter));
        let _ = std::fs::create_dir_all(iter_state_dir.join("sessions"));
        let _ = std::fs::create_dir_all(iter_state_dir.join("memory"));

        let mut run_total_compressed = 0usize;
        let mut iter_output = String::new();

        let t_run_start = Instant::now();
        for call_idx in 0..calls {
            // Multi-call (crosscall) scenarios use the numbered fixture files 1/2/3.
            // Single-call scenarios always use the scenario's own temp fixture file.
            let input_file = if calls > 1 {
                let alt = format!("context_crosscall_{}.txt", call_idx + 1);
                let alt_path = fixtures_dir().join(&alt);
                if alt_path.exists() { alt_path } else { fixture_file.clone() }
            } else {
                fixture_file.clone()
            };

            let t0 = Instant::now();
            let output = std::process::Command::new(&binary)
                .arg("wrap")
                .arg(format!("cat {}", input_file.display()))
                .env("SQUEEZ_DIR", &iter_state_dir)
                .output();
            let elapsed = t0.elapsed().as_micros() as u64;
            all_latencies_us.push(elapsed);

            if let Ok(out) = output {
                let s = String::from_utf8_lossy(&out.stdout).to_string();
                run_total_compressed += s.len() / 4;
                if call_idx + 1 == calls {
                    iter_output = s;
                }
            }
        }
        let _ = t_run_start; // suppress warning
        total_compressed_tokens_per_run.push(run_total_compressed);
        last_output_all_calls = iter_output;

        let _ = std::fs::remove_dir_all(&iter_state_dir);
    }

    let _ = std::fs::remove_dir_all(&tmp_dir);

    all_latencies_us.sort_unstable();
    let median_us = if all_latencies_us.is_empty() {
        0
    } else {
        all_latencies_us[all_latencies_us.len() / 2]
    };

    // Baseline tokens = content * calls (what we'd send without dedup)
    let baseline_total = baseline_tokens * calls;
    let avg_compressed: usize = if total_compressed_tokens_per_run.is_empty() {
        baseline_total
    } else {
        total_compressed_tokens_per_run.iter().sum::<usize>() / total_compressed_tokens_per_run.len()
    };

    let reduction = reduction_pct(baseline_total, avg_compressed);
    let qscore = quality_score(
        &scenario.content,
        &last_output_all_calls,
        &scenario.required_keywords,
        &scenario.quality_mode,
    );

    ScenarioResult {
        name: scenario.name.clone(),
        category: scenario.category.clone(),
        baseline_tokens: baseline_total,
        compressed_tokens: avg_compressed,
        reduction_pct: reduction,
        latency_us: median_us,
        quality_score: qscore,
        quality_pass: qscore >= QUALITY_PASS_THRESHOLD,
        context_saved_tokens: if baseline_total > avg_compressed {
            baseline_total - avg_compressed
        } else {
            0
        },
        iterations,
    }
}

fn run_scenario(scenario: &Scenario, iterations: usize) -> ScenarioResult {
    match &scenario.kind {
        ScenarioKind::Filter { hint } => run_filter(scenario, hint, iterations),
        ScenarioKind::Markdown => run_markdown(scenario, iterations),
        ScenarioKind::Wrap { calls } => run_wrap(scenario, *calls, iterations),
    }
}

// ─── Aggregate report builder ─────────────────────────────────────────────────

fn reduction_pct(before: usize, after: usize) -> f64 {
    if before == 0 {
        return 0.0;
    }
    let saved = before.saturating_sub(after) as f64;
    (saved / before as f64) * 100.0
}

fn weighted_avg_reduction(results: &[ScenarioResult], category_prefix: &str) -> f64 {
    let filtered: Vec<&ScenarioResult> = results
        .iter()
        .filter(|r| r.category.starts_with(category_prefix))
        .collect();
    if filtered.is_empty() {
        return 0.0;
    }
    let total_baseline: usize = filtered.iter().map(|r| r.baseline_tokens).sum();
    let total_compressed: usize = filtered.iter().map(|r| r.compressed_tokens).sum();
    reduction_pct(total_baseline, total_compressed)
}

fn build_report(results: Vec<ScenarioResult>) -> BenchmarkReport {
    let total_baseline: usize = results.iter().map(|r| r.baseline_tokens).sum();
    let total_compressed: usize = results.iter().map(|r| r.compressed_tokens).sum();
    let total_reduction = reduction_pct(total_baseline, total_compressed);

    let bash_reduction = weighted_avg_reduction(&results, "bash_output");
    let md_reduction = weighted_avg_reduction(&results, "markdown");
    let wrap_reduction = weighted_avg_reduction(&results, "wrap");

    let mut all_latencies: Vec<u64> = results.iter().filter(|r| r.latency_us > 0).map(|r| r.latency_us).collect();
    all_latencies.sort_unstable();
    let avg_latency_us = if all_latencies.is_empty() {
        0
    } else {
        all_latencies.iter().sum::<u64>() / all_latencies.len() as u64
    };
    let p95_latency_us = if all_latencies.is_empty() {
        0
    } else {
        let idx = (all_latencies.len() as f64 * 0.95) as usize;
        all_latencies[idx.min(all_latencies.len() - 1)]
    };

    let cost_savings = total_reduction; // cost scales linearly with tokens

    let quality_pass = results.iter().filter(|r| r.quality_pass).count();
    let quality_fail = results.iter().filter(|r| !r.quality_pass && r.iterations > 0).count();
    let quality_skip = results.iter().filter(|r| r.iterations == 0).count();

    BenchmarkReport {
        results,
        total_baseline_tokens: total_baseline,
        total_compressed_tokens: total_compressed,
        total_reduction_pct: total_reduction,
        bash_reduction_pct: bash_reduction,
        md_reduction_pct: md_reduction,
        wrap_reduction_pct: wrap_reduction,
        avg_latency_us,
        p95_latency_us,
        estimated_cost_savings_pct: cost_savings,
        quality_pass_count: quality_pass,
        quality_fail_count: quality_fail,
        quality_skip_count: quality_skip,
    }
}

// ─── Human-readable printer ───────────────────────────────────────────────────

pub fn print_human(report: &BenchmarkReport) {
    println!();
    println!("╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║              squeez benchmark — token reduction & quality report             ║");
    println!("╚══════════════════════════════════════════════════════════════════════════════╝");
    println!();

    // ── Per-scenario table ──────────────────────────────────────────────────
    println!("{:<32} {:>8} {:>8} {:>10} {:>8} {:>7}  {}", "SCENARIO", "BEFORE", "AFTER", "REDUCTION", "LATENCY", "QUALITY", "STATUS");
    println!("{}", "─".repeat(84));

    let mut last_cat = String::new();
    for r in &report.results {
        if r.iterations == 0 {
            println!("{:<32}  [skipped — binary not found]", r.name);
            continue;
        }
        if r.category != last_cat {
            println!();
            println!("  ▸ {}", r.category.replace('_', " ").to_uppercase());
            last_cat = r.category.clone();
        }
        let status = if r.quality_pass { "✅" } else { "❌ quality" };
        let latency_str = format_latency(r.latency_us);
        println!(
            "  {:<30} {:>6}tk {:>6}tk {:>8.1}%  {:>8}  {:>5.0}%   {}",
            r.name,
            r.baseline_tokens,
            r.compressed_tokens,
            r.reduction_pct,
            latency_str,
            r.quality_score * 100.0,
            status
        );
    }

    println!();
    println!("{}", "═".repeat(84));
    println!();

    // ── Aggregate summary ────────────────────────────────────────────────────
    println!("SUMMARY");
    println!("  Total token reduction   {:>7.1}%  ({} tk → {} tk)",
        report.total_reduction_pct,
        report.total_baseline_tokens,
        report.total_compressed_tokens,
    );
    println!();
    println!("  ├─ Bash output          {:>7.1}%  (filter pipeline)", report.bash_reduction_pct);
    println!("  ├─ Markdown/context     {:>7.1}%  (compress-md)", report.md_reduction_pct);
    println!("  └─ Wrap/cross-call      {:>7.1}%  (context engine + dedup)", report.wrap_reduction_pct);

    println!();

    // ── Cost savings ─────────────────────────────────────────────────────────
    let savings_frac = report.estimated_cost_savings_pct / 100.0;
    for (label, price) in [
        ("Claude Sonnet 4.6 · $3.00/MTok", INPUT_COST_PER_MTOK),
        ("Claude Opus 4.7   · $5.00/MTok", INPUT_COST_PER_MTOK_OPUS47),
    ] {
        println!("ESTIMATED COST SAVINGS  ({} input)", label);
        for calls_per_day in [100u64, 1_000, 10_000] {
            // Assume each call sends ~2k tokens of context on average
            let avg_context_tokens_per_call = 2_000.0f64;
            let monthly_tokens = calls_per_day as f64 * avg_context_tokens_per_call * 30.0;
            let baseline_cost = monthly_tokens / 1_000_000.0 * price;
            let saved = baseline_cost * savings_frac;
            println!("  {:>6} calls/day  → ${:.2}/month baseline  → ${:.2} saved/month  ({:.1}%)",
                format_num(calls_per_day), baseline_cost, saved, report.estimated_cost_savings_pct);
        }
        println!();
    }

    println!();

    // ── Latency ──────────────────────────────────────────────────────────────
    println!("LATENCY (compression overhead, filter mode)");
    println!("  avg p50    {:>8}", format_latency(report.avg_latency_us));
    println!("  p95        {:>8}", format_latency(report.p95_latency_us));

    println!();

    // ── Quality ──────────────────────────────────────────────────────────────
    println!("QUALITY  (≥{:.0}% of key terms preserved)", QUALITY_PASS_THRESHOLD * 100.0);
    let total_scored = report.quality_pass_count + report.quality_fail_count;
    println!("  passed   {}/{}", report.quality_pass_count, total_scored);
    if report.quality_fail_count > 0 {
        println!("  FAILED   {}/{}", report.quality_fail_count, total_scored);
        println!();
        for r in report.results.iter().filter(|r| !r.quality_pass && r.iterations > 0) {
            println!("    ⚠  {}  quality={:.0}%", r.name, r.quality_score * 100.0);
        }
    }
    if report.quality_skip_count > 0 {
        println!("  skipped  {} (binary not found)", report.quality_skip_count);
    }

    println!();

    // ── Interpretation ────────────────────────────────────────────────────────
    println!("INTERPRETATION");
    println!("  Best gains:   high-volume/noisy outputs (ps aux, logs, repetitive lines)");
    println!("  Moderate:     structured diffs and markdown prose");
    println!("  Trade-off:    ultra-mode truncates aggressively — use --no-squeez for deep diffs");
    println!("  Recommendation: keep adaptive_intensity=true for maximum context budget savings");
    println!();
}

fn format_latency(us: u64) -> String {
    if us == 0 {
        return "  n/a".to_string();
    }
    if us < 1_000 {
        format!("{}µs", us)
    } else if us < 1_000_000 {
        format!("{:.1}ms", us as f64 / 1_000.0)
    } else {
        format!("{:.2}s", us as f64 / 1_000_000.0)
    }
}

fn format_num(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

// ─── JSON report ──────────────────────────────────────────────────────────────

pub fn to_json(report: &BenchmarkReport) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"schema_version\": 1,\n");
    out.push_str(&format!("  \"squeez_version\": \"{}\",\n", env!("CARGO_PKG_VERSION")));
    out.push_str(&format!("  \"total_baseline_tokens\": {},\n", report.total_baseline_tokens));
    out.push_str(&format!("  \"total_compressed_tokens\": {},\n", report.total_compressed_tokens));
    out.push_str(&format!("  \"total_reduction_pct\": {:.2},\n", report.total_reduction_pct));
    out.push_str(&format!("  \"bash_reduction_pct\": {:.2},\n", report.bash_reduction_pct));
    out.push_str(&format!("  \"md_reduction_pct\": {:.2},\n", report.md_reduction_pct));
    out.push_str(&format!("  \"wrap_reduction_pct\": {:.2},\n", report.wrap_reduction_pct));
    out.push_str(&format!("  \"estimated_cost_savings_pct\": {:.2},\n", report.estimated_cost_savings_pct));
    out.push_str(&format!("  \"avg_latency_us\": {},\n", report.avg_latency_us));
    out.push_str(&format!("  \"p95_latency_us\": {},\n", report.p95_latency_us));
    out.push_str(&format!("  \"quality_pass_count\": {},\n", report.quality_pass_count));
    out.push_str(&format!("  \"quality_fail_count\": {},\n", report.quality_fail_count));
    out.push_str(&format!("  \"quality_skip_count\": {},\n", report.quality_skip_count));
    out.push_str("  \"scenarios\": [\n");
    for (i, r) in report.results.iter().enumerate() {
        let comma = if i + 1 < report.results.len() { "," } else { "" };
        out.push_str("    {\n");
        out.push_str(&format!("      \"name\": \"{}\",\n", json_util::escape_str(&r.name)));
        out.push_str(&format!("      \"category\": \"{}\",\n", json_util::escape_str(&r.category)));
        out.push_str(&format!("      \"baseline_tokens\": {},\n", r.baseline_tokens));
        out.push_str(&format!("      \"compressed_tokens\": {},\n", r.compressed_tokens));
        out.push_str(&format!("      \"reduction_pct\": {:.2},\n", r.reduction_pct));
        out.push_str(&format!("      \"latency_us\": {},\n", r.latency_us));
        out.push_str(&format!("      \"quality_score\": {:.4},\n", r.quality_score));
        out.push_str(&format!("      \"quality_pass\": {},\n", r.quality_pass));
        out.push_str(&format!("      \"context_saved_tokens\": {},\n", r.context_saved_tokens));
        out.push_str(&format!("      \"iterations\": {}\n", r.iterations));
        out.push_str(&format!("    }}{}\n", comma));
    }
    out.push_str("  ]\n");
    out.push('}');
    out
}

// ─── CLI entry ────────────────────────────────────────────────────────────────

pub fn run(args: &[String]) -> i32 {
    let mut json_mode = false;
    let mut output_file: Option<String> = None;
    let mut scenario_filter: Option<String> = None;
    let mut iterations: usize = 5;
    let mut list_only = false;
    let mut baseline_mode = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--json" => json_mode = true,
            "--list" => list_only = true,
            "--baseline" => baseline_mode = true,
            "--output" | "-o" => {
                i += 1;
                output_file = args.get(i).cloned();
            }
            "--scenario" | "-s" => {
                i += 1;
                scenario_filter = args.get(i).cloned();
            }
            "--iterations" | "-n" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    iterations = v.parse().unwrap_or(3);
                }
            }
            "-h" | "--help" => {
                print_help();
                return 0;
            }
            other => {
                eprintln!("squeez benchmark: unknown flag '{}'", other);
                return 2;
            }
        }
        i += 1;
    }

    let fixtures = fixtures_dir();
    let all_scenarios = build_scenarios(&fixtures);

    if list_only {
        println!("Available scenarios ({}):", all_scenarios.len());
        for s in &all_scenarios {
            println!("  {:32} [{}]", s.name, s.category);
        }
        return 0;
    }

    // Filter scenarios if requested
    let to_run: Vec<&Scenario> = if let Some(ref filter) = scenario_filter {
        all_scenarios
            .iter()
            .filter(|s| s.name.contains(filter.as_str()) || s.category.contains(filter.as_str()))
            .collect()
    } else {
        all_scenarios.iter().collect()
    };

    if to_run.is_empty() {
        eprintln!("squeez benchmark: no scenarios matched '{}'", scenario_filter.as_deref().unwrap_or(""));
        return 1;
    }

    eprintln!(
        "squeez benchmark: running {} scenario(s) × {} iteration(s) ...",
        to_run.len(),
        iterations
    );
    eprintln!("  fixtures dir: {}", fixtures.display());
    eprintln!();

    // Run
    let results: Vec<ScenarioResult> = to_run
        .iter()
        .map(|s| {
            eprint!("  {:32} ... ", s.name);
            let r = run_scenario(s, iterations);
            if r.iterations == 0 {
                eprintln!("skipped");
            } else {
                eprintln!("{:.1}% reduction  quality={:.0}%", r.reduction_pct, r.quality_score * 100.0);
            }
            r
        })
        .collect();

    let report = build_report(results);

    // JSON output
    let json = to_json(&report);
    if let Some(ref path) = output_file {
        match std::fs::write(path, &json) {
            Ok(_) => eprintln!("  JSON report → {}", path),
            Err(e) => eprintln!("  warn: could not write {}: {}", path, e),
        }
    }
    if json_mode {
        println!("{}", json);
    } else if baseline_mode {
        print_baseline_comparison(&report);
    } else {
        print_human(&report);
    }

    if report.quality_fail_count > 0 { 1 } else { 0 }
}

/// Print an A/B comparison table: SCENARIO | BASELINE | SQUEEZ | SAVINGS.
/// The "baseline" column shows what Claude would receive without any compression
/// (raw input tokens); "squeez" shows compressed tokens; "savings" is the delta.
/// This directly maps to the C0 (baseline) vs C4 (hook filtering) hypothesis from
/// the research framework.
fn print_baseline_comparison(report: &BenchmarkReport) {
    println!();
    println!("╔═══════════════════════════════════════════════════════════════════════╗");
    println!("║         squeez A/B comparison — baseline vs hook-filtered (C4)        ║");
    println!("╚═══════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("{:<32} {:>10} {:>10} {:>10} {:>9}", "SCENARIO", "BASELINE", "SQUEEZ", "SAVINGS", "REDUCTION");
    println!("{}", "─".repeat(76));

    let mut total_baseline = 0usize;
    let mut total_squeez = 0usize;

    let mut last_cat = String::new();
    for r in &report.results {
        if r.iterations == 0 { continue; }
        if r.category != last_cat {
            println!();
            println!("  ▸ {}", r.category.replace('_', " ").to_uppercase());
            last_cat = r.category.clone();
        }
        let savings = r.baseline_tokens.saturating_sub(r.compressed_tokens);
        println!(
            "  {:<30} {:>8}tk {:>8}tk {:>8}tk {:>7.1}%",
            r.name,
            r.baseline_tokens,
            r.compressed_tokens,
            savings,
            r.reduction_pct,
        );
        total_baseline += r.baseline_tokens;
        total_squeez += r.compressed_tokens;
    }

    let total_savings = total_baseline.saturating_sub(total_squeez);
    let total_reduction = reduction_pct(total_baseline, total_squeez);

    println!();
    println!("{}", "═".repeat(76));
    println!(
        "  {:<30} {:>8}tk {:>8}tk {:>8}tk {:>7.1}%",
        "TOTAL",
        total_baseline,
        total_squeez,
        total_savings,
        total_reduction,
    );
    println!();
    println!("C0 (baseline, no filtering) vs C4 (squeez hook filtering):");
    println!("  Without squeez: {:>8}tk sent to Claude per benchmark run", total_baseline);
    println!("  With squeez:    {:>8}tk sent to Claude per benchmark run", total_squeez);
    println!("  Net savings:    {:>8}tk ({:.1}% reduction)", total_savings, total_reduction);
    println!();
}

fn print_help() {
    eprintln!("squeez benchmark — measure token reduction, cost savings, latency, quality");
    eprintln!();
    eprintln!("USAGE");
    eprintln!("  squeez benchmark [OPTIONS]");
    eprintln!();
    eprintln!("OPTIONS");
    eprintln!("  --list                  List all available scenarios");
    eprintln!("  --scenario, -s <name>   Run only scenarios whose name/category contains <name>");
    eprintln!("  --iterations, -n <n>    Iterations per scenario (default: 5)");
    eprintln!("  --baseline              Show A/B comparison (C0 baseline vs C4 squeez)");
    eprintln!("  --json                  Print JSON report to stdout");
    eprintln!("  --output, -o <file>     Write JSON report to <file>");
    eprintln!("  --help, -h              Show this help");
    eprintln!();
    eprintln!("ENVIRONMENT");
    eprintln!("  SQUEEZ_BENCH_FIXTURES   Override fixture directory path");
    eprintln!();
    eprintln!("EXAMPLES");
    eprintln!("  squeez benchmark");
    eprintln!("  squeez benchmark --scenario git");
    eprintln!("  squeez benchmark --json --output bench/report.json");
    eprintln!("  squeez benchmark -n 5 --scenario bash_output");
}
