# Changelog

All notable changes to squeez are documented here.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed
- `squeez update` on Windows now detects cargo-managed installs and delegates to `cargo install squeez` automatically, avoiding exe-lock issues
- `install_target_path()` uses `current_exe()` instead of hardcoded `~/.claude/squeez/bin`, so the correct binary is always updated regardless of install method
- Windows self-update fallback spawns a detached `cmd.exe` to move the staged binary after process exit, instead of requiring manual `move /Y`
- `install_atomic` now returns `Ok(bool)` distinguishing immediate vs deferred installs; success message no longer appears when install was only staged

### Added
- `session-start.sh`: rate-limited update check (once per day) — injects `[squeez] Update available` notification into Claude session context when a new version is detected

## [0.3.0] - 2026-04-14

### Added
- Token economy layer: sub-agent cost tracking via PostToolUse hook (~200K tokens/spawn estimate)
- Context pressure prediction: sliding-window burn rate with `[budget: ~N calls left]` header
- Session efficiency scoring: compression ratio, tool choice, context reuse, budget conservation (basis points)
- Tool result size budgets: per-tool output caps via PreToolUse hook (`read_max_lines`, `grep_max_results`)
- Auto-calibration on install: `squeez calibrate` runs benchmarks and generates optimized `config.ini`
- 2 new MCP tools: `squeez_agent_costs`, `squeez_session_efficiency` (11 → 13 total)
- 2 new CLI commands: `squeez calibrate`, `squeez budget-params <tool>`
- 5 new config keys: `agent_warn_threshold_pct`, `burn_rate_warn_calls`, `agent_spawn_cost`, `read_max_lines`, `grep_max_results`
- 29 new economy module unit tests
- `src/economy/` module: `agent_tracker.rs`, `burn_rate.rs`, `efficiency.rs`, `calibrate.rs`, `budget.rs`

### Changed
- PreToolUse hook now matches all tools (was Bash-only) for budget enforcement
- `build.sh` runs `squeez calibrate` automatically after install
- Wrap header now shows burn rate and agent cost warnings
- Session finalization computes and stores efficiency scores in `memory/summaries.jsonl`
- `SessionContext` extended with agent tracking and burn window fields

## [0.2.3] - 2026-04-10

### Added
- Structured session memory: `investigated`, `learned`, `completed`, `next_steps` fields in summaries
- Cross-session search: `squeez_search_history`, `squeez_file_history` MCP tools
- Error snippets: first 128 chars stored alongside FNV fingerprints
- Configurable tunables: `max_call_log`, `recent_window`, `similarity_threshold`, `ultra_trigger_pct`
- `squeez_seen_error_details`, `squeez_session_detail`, `squeez_session_stats` MCP tools (6 → 11 total)
- Locale-aware `compress-md` with pt-BR support (`lang = pt` in config or `--lang pt`)
- `config.ini` auto-created on first install via `squeez setup`

### Changed
- MCP tool count: 6 → 11
- README rewritten with full install instructions and all commands

### Fixed
- Windows setup via cargo/npm + `squeez update` fix
- CI: dispatch `release.yml` from `auto-release` to bypass GITHUB_TOKEN chain limit

## [0.2.2] - 2026-03-28

### Added
- `squeez benchmark` command: 19-scenario reproducible benchmark suite with quality scoring
- JSON output mode (`--json`) and scenario filtering (`--scenario`)

### Changed
- README rewritten with full install instructions (curl/npm/cargo)

### Fixed
- Flaky test tmp dir collision in CI
- `bench/run.sh` excludes synthetic fixtures (filter-mode only)
- CI workflow registration fixes for `promote-develop`

## [0.2.1] - 2026-03-20

### Added
- Persona injection into `~/.claude/CLAUDE.md` on every session start
- npm package and crates.io publish jobs in release workflow
- Statusline with real-time compression metrics

### Fixed
- `compress_md`: byte slice comparison to avoid UTF-8 char boundary panic
- Statusline: proper capitalization, green checkmark, real-time context %

## [0.2.0] - 2026-03-15

### Added
- Context engine: adaptive intensity, redundancy cache, summarize fallback
- `compress-md` command: pure-Rust markdown compressor
- Caveman persona prompt injection
- `squeez update` self-update command
- `track-result` PostToolUse hook for file/error tracking
- Cloud CLI handlers (kubectl, gh, aws, gcloud, az)
- Database handlers (psql, prisma, mysql)
- Data tool handlers (jq, yq, terraform)
- Windows support in `install.sh` and `release.yml`

### Changed
- Wider redundancy window, token budget improvements
- CI: PR template, linked-issue enforcement

[Unreleased]: https://github.com/claudioemmanuel/squeez/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/claudioemmanuel/squeez/compare/v0.2.3...v0.3.0
[0.2.3]: https://github.com/claudioemmanuel/squeez/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/claudioemmanuel/squeez/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/claudioemmanuel/squeez/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/claudioemmanuel/squeez/releases/tag/v0.2.0
