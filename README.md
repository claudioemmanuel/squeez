<p align="center">
  <img src="assets/banner.png" alt="squeez — hook-based token compressor for AI CLIs" width="100%">
</p>

# squeez

[![CI](https://github.com/claudioemmanuel/squeez/actions/workflows/ci.yml/badge.svg)](https://github.com/claudioemmanuel/squeez/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/squeez.svg)](https://www.npmjs.com/package/squeez)
[![Crates.io](https://img.shields.io/crates/v/squeez.svg)](https://crates.io/crates/squeez)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![DCO](https://img.shields.io/badge/contributions-DCO_signed--off-green.svg)](CONTRIBUTING.md#license--contributor-sign-off)
[![Changelog](https://img.shields.io/badge/changelog-📋-blue.svg)](CHANGELOG.md)

End-to-end token optimizer for Claude Code, GitHub Copilot CLI, OpenCode, Gemini CLI, and OpenAI Codex CLI. Compresses bash output up to **95%**, collapses redundant calls, and injects a terse prompt persona — automatically, with zero new runtime dependencies.

---

## Install

Three methods — all produce the same result (binary at `~/.claude/squeez/bin/squeez`, hooks registered).

### curl (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/claudioemmanuel/squeez/main/install.sh | sh
```

> **Windows:** requires [Git Bash](https://git-scm.com/downloads). Run the command above inside Git Bash — PowerShell/CMD are not supported.

### npm / npx

```bash
# Install globally
npm install -g squeez

# Or run once without installing
npx squeez
```

Downloads the correct pre-built binary for your platform (macOS universal, Linux x86_64/aarch64, Windows x86_64). Requires Node ≥ 16.

### cargo (build from source)

```bash
cargo install squeez
```

Builds from [crates.io](https://crates.io/crates/squeez). Requires Rust stable. On Windows you also need [MSVC C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/).

---

### Supported hosts

`squeez setup` auto-detects every CLI present on disk and registers the hooks. `squeez uninstall` removes them. Session data and `config.ini` are preserved so reinstall is lossless.

| Host | Memory file | Bash wrap | Session memory | Budget inject (Read/Grep) | Notes |
|---|---|---|---|---|---|
| **Claude Code** | `~/.claude/CLAUDE.md` | ✅ native | ✅ native | ✅ native | Restart Claude Code to pick up hooks |
| **Copilot CLI** | `~/.copilot/copilot-instructions.md` | ✅ native | ✅ native | ✅ native | Restart Copilot CLI after setup |
| **OpenCode** | `~/.config/opencode/AGENTS.md` | ✅ native | ✅ native | ✅ native | Plugin at `~/.config/opencode/plugins/squeez.js`; MCP tool calls skip hooks (upstream sst/opencode#2319) |
| **Gemini CLI** | `~/.gemini/GEMINI.md` | ✅ native | ✅ native | 🟡 soft via `GEMINI.md` | `BeforeTool` rewrite schema pending upstream docs ([google-gemini/gemini-cli#25629](https://github.com/google-gemini/gemini-cli/issues/25629)) |
| **Codex CLI** | `~/.codex/AGENTS.md` | ✅ native | ✅ native | 🟡 soft via `AGENTS.md` | Codex `PreToolUse` is Bash-only until upstream expands ([openai/codex#18491](https://github.com/openai/codex/issues/18491)) |

### Manage

```bash
squeez setup                  # register into every detected host
squeez setup --host=<slug>    # register into one host
squeez uninstall              # remove squeez entries from every detected host
squeez uninstall --host=<slug>
```

Slugs: `claude-code` / `copilot` / `opencode` / `gemini` / `codex`.

After install, restart the CLI you use to pick up the new hooks.

### Uninstall

```bash
squeez uninstall              # preserves session data + config.ini
bash ~/.claude/squeez/uninstall.sh   # (legacy) full wipe, if the script exists
```

### Self-update

```bash
squeez update             # download latest binary + verify SHA256
squeez update --check     # check for update without installing
squeez update --insecure  # skip checksum (not recommended)
```

---

## What it does

| Feature | Description |
|---------|-------------|
| **Bash compression** | Intercepts every command via `PreToolUse` hook, applies smart filter → dedup → grouping → truncation. Up to 95% reduction. |
| **Context engine** | Cross-call redundancy with two paths: exact-hash match (FNV-1a, fast) **and** fuzzy trigram-shingle Jaccard ≥0.85 (whitespace, timestamps, single-line edits no longer defeat dedup). |
| **Summarize fallback** | Outputs exceeding 500 lines are replaced with a ≤40-line dense summary (top errors, files, test result, tail). **Benign outputs get 2× the threshold** so successful builds stay verbatim. |
| **Adaptive intensity** | Truly adaptive: **Full** (×0.6 limits) below 80% of token budget, **Ultra** (×0.3) above. Used to be always-Ultra; now actually responds to session pressure. |
| **MCP server** | `squeez mcp` runs a JSON-RPC 2.0 server over stdio exposing 13 read-only tools so any MCP-compatible LLM can query session memory directly. Hand-rolled, no `mcp.server` dependency. |
| **Auto-teach payload** | `squeez protocol` (or the `squeez_protocol` MCP tool) prints a 2.4 KB self-describing payload — the LLM learns squeez's markers and protocol on first call. |
| **Caveman persona** | Injects an ultra-terse prompt at session start so the model responds with fewer tokens. |
| **Memory-file compression** | `squeez compress-md` compresses CLAUDE.md / AGENTS.md / copilot-instructions.md in-place — pure Rust, zero LLM. i18n-aware: set `lang = pt` (or `--lang pt`) for pt-BR article/filler/phrase dropping and Unicode-correct matching. |
| **Session memory** | On `SessionStart`, injects a structured summary of the previous session: files investigated, learned facts (errors + git events), completed work (builds, test passes), and next steps (unresolved errors, failing tests). Summaries carry temporal validity (`valid_from`/`valid_to`). |
| **Token tracking** | Every `PostToolUse` result (Bash, Read, Grep, Glob) feeds a `SessionContext` so squeez knows what the agent has already seen. |
| **Token economy** | Sub-agent cost tracking (~200K tokens/spawn), burn rate prediction (`[budget: ~N calls left]`), session efficiency scoring, tool result size budgets. |
| **Auto-calibration** | `squeez calibrate` runs benchmarks on install and generates an optimized `config.ini` (aggressive / balanced / conservative profiles). |

---

## Benchmarks

<!-- BENCHMARK:START -->
Measured on macOS (Apple Silicon). Token count = `chars / 4` (matches Claude's ~4 chars/token). Run `squeez benchmark` to reproduce.

### Per-scenario results — 23 scenarios × 5 iterations

| Scenario | Before | After | Reduction | Latency |
|----------|--------|-------|-----------|---------|
| `summarize_huge` | 82,257 tk | 420 tk | **-99%** | 54.6 ms |
| `repetitive_output` | 4,692 tk | 37 tk | **-99%** | 164 µs |
| `high_context_adaptive` | 4,418 tk | 52 tk | **-99%** | 637 µs |
| `ps_aux` | 40,373 tk | 2,352 tk | **-94%** | 2.2 ms |
| `git_log_200` | 2,692 tk | 289 tk | **-89%** | 179 µs |
| `tsc_errors` | 731 tk | 101 tk | **-86%** | 20 µs |
| `cargo_build_noisy` | 2,106 tk | 452 tk | **-79%** | 201 µs |
| `docker_logs` | 665 tk | 186 tk | **-72%** | 36 µs |
| `find_deep` | 424 tk | 134 tk | **-68%** | 61 µs |
| `git_status` | 50 tk | 16 tk | **-68%** | 9 µs |
| `state_first_simulation` | 182 tk | 69 tk | **-62%** | 9 µs |
| `verbose_app_log` | 4,957 tk | 1,991 tk | **-60%** | 235 µs |
| `npm_install` | 524 tk | 232 tk | **-56%** | 38 µs |
| `claude_md_overhead` | 717 tk | 318 tk | **-56%** | 272 µs |
| `crosscall_redundancy_3x` | 486 tk | 241 tk | **-50%** | 51.2 ms |
| `ls_la` | 1,782 tk | 886 tk | **-50%** | 168 µs |
| `env_dump` | 441 tk | 287 tk | **-35%** | 19 µs |
| `git_copilot` | 640 tk | 421 tk | **-34%** | 88 µs |
| `agent_heavy` | 2,306 tk | 1,564 tk | **-32%** | 310 µs |
| `md_prose` | 187 tk | 138 tk | **-26%** | 505 µs |
| `md_claude_md` | 316 tk | 247 tk | **-22%** | 938 µs |
| `git_diff` | 502 tk | 497 tk | **-1%** | 35 µs |
| `kubectl_pods` | 1,513 tk | 1,513 tk | **-0%** | 20 µs |

### Aggregate

| Metric | Value |
|--------|-------|
| **Total token reduction** | **91.9%** — 152,961 tk → 12,443 tk |
| Bash output | **-84.9%** |
| Markdown / context files | **-23.5%** |
| Wrap / cross-call engine | **-99.2%** |
| Quality (signal terms preserved) | **23 / 23 pass** |
| Latency p50 (filter mode) | **4.9 ms** |
| Latency p95 (incl. wrap/summarize) | **51 ms** |

### Estimated cost savings — Claude Sonnet 4.6 · $3.00 / MTok input

| Usage | Baseline / month | Saved / month |
|-------|-----------------|---------------|
| 100 calls / day | $18.00 | **$16.54 (92%)** |
| 1,000 calls / day | $180.00 | **$165.37 (92%)** |
| 10,000 calls / day | $1800.00 | **$1653.66 (92%)** |
<!-- BENCHMARK:END -->

---

## Commands

```bash
squeez wrap <cmd>                        # compress a command's output end-to-end
squeez filter <hint>                     # compress stdin (piped usage)
squeez compress-md [--ultra] [--dry-run] [--all] <file>...   # compress markdown files
squeez benchmark [--json] [--output <file>] [--scenario <name>] [--iterations <n>]
squeez mcp                               # JSON-RPC 2.0 MCP server over stdin/stdout
squeez protocol                          # print the auto-teach payload (markers + protocol)
squeez update [--check] [--insecure]     # self-update
squeez init [--copilot]                  # session-start hook (called by hook, not manually)
squeez calibrate                         # auto-tune config from benchmarks
squeez budget-params <tool>              # output JSON budget patch for tool
squeez --version
```

### Escape hatch — bypass compression for one command

```bash
--no-squeez git log --all --graph
```

Prefix any command with `--no-squeez` to run it raw without squeez touching it.

### `squeez wrap`

Runs a command, compresses its output, and prints a savings header:

```
# squeez [git log] 2692→289 tokens (-89%) 0.2ms [adaptive: Ultra]
```

### `squeez filter`

Reads from stdin. Use for manual pipelines:

```bash
git log --oneline | squeez filter git
docker logs mycontainer 2>&1 | squeez filter docker
```

### `squeez compress-md`

Pure-Rust, zero-LLM compressor for markdown files. Preserves code blocks, inline code, URLs, headings, file paths, and tables. Compresses prose only. Always writes a backup at `<stem>.original.md`.

```bash
squeez compress-md CLAUDE.md             # Full mode (English default)
squeez compress-md --ultra CLAUDE.md    # + abbreviations (with→w/, fn, cfg, etc.)
squeez compress-md --lang pt CLAUDE.md  # pt-BR locale (articles, fillers, phrases)
squeez compress-md --dry-run CLAUDE.md  # preview, no write
squeez compress-md --all                # compress all known locations automatically
```

When `auto_compress_md = true` (default), `squeez init` runs `--all` silently on every session start.

### `squeez benchmark`

Reproducible measurement of token reduction, cost, latency, and quality across 19 scenarios:

```bash
squeez benchmark                          # human-readable report
squeez benchmark --json                   # JSON to stdout
squeez benchmark --output report.json     # save JSON report
squeez benchmark --scenario git           # run only git scenarios
squeez benchmark --iterations 5           # more iterations per scenario
squeez benchmark --list                   # list all scenarios
```

Quality is scored by checking that **signal terms** (words from error/warning/failed lines in the baseline) survive compression. 19/19 pass at ≥ 50% threshold.

### `squeez mcp`

Runs a Model Context Protocol JSON-RPC 2.0 server over stdin/stdout. Hand-rolled, no `mcp.server` / `fastmcp` dependency — keeps the `libc`-only constraint intact. Wire it into Claude Code:

```bash
claude mcp add squeez -- /path/to/squeez mcp
```

Thirteen read-only tools become available to the LLM:

| Tool | Returns |
|------|---------|
| `squeez_recent_calls` | Last N bash invocations with hash + length + cmd snippet — check before re-running |
| `squeez_seen_files` | Files this session has touched, with access type (Read/Write/Created/Deleted), sorted by recency |
| `squeez_seen_errors` | Distinct error fingerprints observed this session (FNV-1a hashes of normalized errors) |
| `squeez_seen_error_details` | Error fingerprints with the first 128 chars of message text — find *what* the error was |
| `squeez_session_summary` | Token accounting + call counts (tokens_bash / tokens_read / tokens_other / seen_files / seen_errors / seen_git_refs) |
| `squeez_session_stats` | Dedup hit counts (exact + fuzzy), summarize triggers, Ultra-mode calls, tokens saved per category |
| `squeez_agent_costs` | Sub-agent usage: spawn count, cumulative estimated tokens, per-call breakdown |
| `squeez_session_efficiency` | Session efficiency scores: compression ratio, tool choice, context reuse, budget conservation (basis points) |
| `squeez_prior_summaries` | Last N finalized prior-session summaries with structured fields: investigated / learned / completed / next_steps |
| `squeez_search_history` | Full-text search across all session summaries — find when you last saw an error or touched a file |
| `squeez_file_history` | Sessions where a given file path was touched, with token-savings and commit status |
| `squeez_session_detail` | Full structured view of a past session by date: calls, files, errors, git events, test summary |
| `squeez_protocol` | Auto-teach payload — read once per session to learn squeez's markers + memory protocol |

All read-only. Backed by `SessionContext::load()`, `memory::read_last_n()`, and `memory::search_history()`. No side effects.

### `squeez protocol`

Prints the auto-teach payload — a 2.4 KB self-describing block covering:

- The 5-rule **memory protocol** (what to do with `[squeez: ...]` markers, when to call the MCP tools)
- The **output marker spec** (`# squeez [...]`, `[squeez: identical to ...]`, `[squeez: ~95% similar to ...]`, `squeez:summary`, `# squeez hint:`)

Same content the MCP `squeez_protocol` tool returns. Pipe it into a `system` prompt or paste it into a one-shot session that doesn't have the MCP server connected.

---

## Configuration

Optional config file — all fields have defaults, none are required.

| Platform | Config path |
|----------|------------|
| Claude Code / default | `~/.claude/squeez/config.ini` |
| Copilot CLI | `~/.copilot/squeez/config.ini` |

```ini
# ── Compression ────────────────────────────────────────────────
max_lines              = 200     # generic truncation limit
dedup_min              = 3       # collapse lines appearing ≥N times
git_log_max_commits    = 20
git_diff_max_lines     = 150
docker_logs_max_lines  = 100
find_max_results       = 50
bypass                 = docker exec, psql, mysql, ssh   # never compress these

# ── Context engine ─────────────────────────────────────────────
adaptive_intensity         = true    # truly adaptive: Full <80% budget, Ultra ≥80%
context_cache_enabled      = true    # track seen files/errors across calls
redundancy_cache_enabled   = true    # collapse identical OR fuzzy-similar recent outputs
summarize_threshold_lines  = 500     # outputs above this trigger summarize fallback (×2 if benign)
compact_threshold_tokens   = 120000  # session token budget — drives adaptive intensity

# ── Session memory ─────────────────────────────────────────────
memory_retention_days = 30

# ── Output / persona ───────────────────────────────────────────
persona          = ultra    # off | lite | full | ultra
auto_compress_md = true     # run compress-md on every session start
lang             = en       # compress-md locale: en | pt (pt-BR) — more languages extensible

# ── Advanced tuning (rarely needed) ───────────────────────────
max_call_log              = 32    # rolling call log depth (also caps redundancy window)
recent_window             = 16    # how many recent calls are eligible for redundancy lookup
similarity_threshold      = 0.85  # Jaccard threshold for fuzzy dedup (0.0–1.0)
ultra_trigger_pct         = 0.80  # fraction of context budget at which Full → Ultra
mcp_prior_summaries_default = 5   # default n for squeez_prior_summaries
mcp_recent_calls_default    = 10  # default n for squeez_recent_calls

# ── Token economy ─────────────────────────────────────────────
agent_warn_threshold_pct  = 0.50  # warn when agent cost > 50% of budget
burn_rate_warn_calls      = 20    # warn when < 20 calls remaining
agent_spawn_cost          = 200000 # estimated tokens per Agent/Task spawn
read_max_lines            = 0     # max lines injected into Read tool_input (0 = off)
grep_max_results          = 0     # max results injected into Grep tool_input (0 = off)
```

### Adaptive intensity — Full / Ultra split

When `adaptive_intensity = true` (default), squeez **actually adapts** to session pressure rather than always running Ultra:

| Used / budget | Tier | Scaling |
|---|---|---|
| `< 80%` | **Full** | ×0.6 limits, dedup_min ×0.66 (floor 2) |
| `≥ 80%` | **Ultra** | ×0.3 limits, dedup_min ×0.5 (floor 2) |
| `adaptive_intensity = false` | **Lite** | passthrough — no scaling |

Floors are enforced so we never reduce to zero: `max_lines ≥ 20`, `git_diff_max_lines ≥ 20`, `dedup_min ≥ 2`, `summarize_threshold_lines ≥ 50`.

The active level is shown in every bash header: `[adaptive: Full]` or `[adaptive: Ultra]`.

Pre-0.3 squeez was effectively always-Ultra. The new behavior preserves more verbatim text in the common case (empty / mid-session) and only graduates to aggressive compression when the context budget is genuinely under pressure.

### Caveman persona

Three intensity levels (`lite`, `full`, `ultra`) and `off`. Default is `ultra`. The persona prompt is injected into:
- The Claude Code session banner (printed at `SessionStart`)
- The `<!-- squeez:start -->…<!-- squeez:end -->` block in `~/.copilot/copilot-instructions.md` for Copilot CLI

---

## How it works

### Compression pipeline

Each bash command passes through four strategies in order:

1. **smart_filter** — strips ANSI codes, progress bars, spinner chars, timestamps, and tool-specific noise (npm download lines, stack frame noise, etc.)
2. **dedup** — lines appearing ≥ `dedup_min` times are collapsed to one entry annotated `[×N]`
3. **grouping** — files in the same directory (≥5 siblings) are collapsed to `dir/  N modified  [squeez grouped]`
4. **truncation** — `Head` (keep first N) or `Tail` (keep last N) depending on handler; truncated portion noted

### Supported handlers

| Category | Commands |
|----------|----------|
| Git | `git` |
| Docker / containers | `docker`, `docker-compose`, `podman` |
| Package managers | `npm`, `pnpm`, `bun`, `yarn` |
| Build systems | `make`, `cmake`, `gradle`, `mvn`, `xcodebuild`, `cargo` (build), `next build/dev/start` |
| Test runners | `cargo test`, `jest`, `vitest`, `pytest`, `nextest`, `playwright`, `bun test` |
| TypeScript / linters | `tsc`, `eslint`, `biome` |
| Cloud CLIs | `kubectl`, `gh`, `aws`, `gcloud`, `az`, `wrangler` |
| Databases | `psql`, `prisma`, `mysql`, `drizzle-kit` |
| Filesystem | `find`, `ls`, `du`, `ps`, `env`, `lsof`, `netstat` |
| JSON / YAML / IaC | `jq`, `yq`, `terraform`, `tofu`, `helm`, `pulumi` |
| Text processing | `grep`, `rg`, `awk`, `sed` |
| Network | `curl`, `wget` |
| Runtimes | `node`, `python`, `ruby` |
| Generic fallback | everything else |

### Hooks (Claude Code & Copilot CLI)

Three hooks work together automatically after install:

- **`PreToolUse`** — rewrites every Bash call: `git status` → `squeez wrap git status`
- **`SessionStart`** — runs `squeez init`: finalizes previous session into a memory summary, injects the persona prompt
- **`PostToolUse`** — runs `squeez track-result`: scans every tool result (Bash, Read, Grep, Glob) for file paths and errors, feeding `SessionContext`

### Cross-call redundancy

Two-path dedup across the last 16 calls:

**Exact match** — FNV-1a hash of the compressed output. When a subsequent call produces the same bytes, it collapses to:

```
[squeez: identical to 515ba5b2 at bash#35 — re-run with --no-squeez]
```

**Fuzzy match** — bottom-k MinHash over whitespace-token trigrams (k=96, Jaccard ≥ 0.85, length-ratio guard ≥ 0.80). Survives timestamp changes, added/removed blank lines, and single-line edits. Collapses to:

```
[squeez: ~92% similar to 515ba5b2 at bash#35 — re-run with --no-squeez]
```

Minimum 6 lines to attempt fuzzy match (below that, exact-only).

### Summarize fallback

When raw output exceeds `summarize_threshold_lines` (default 500), the full pipeline is bypassed and replaced with a ≤40-line dense summary:

```
squeez:summary cmd=docker logs app
total_lines=5003
top_errors:
  - error: connection refused on tcp://10.0.0.1:5432
top_files:
  - /var/log/app/error.log
test_summary=FAILED: 3 of 248
tail_preserved=20
[last 20 lines verbatim...]
```

**Benign-aware threshold:** before summarizing, squeez scans for error markers (`error:`, `panic`, `traceback`, `FAILED`, `EXCEPTION`, `Fatal`). If none are found, the threshold is doubled (1,000 lines default) so successful builds, clean test runs, and uneventful logs stay verbatim unless they are genuinely huge.

---

## Platform notes

### OpenCode

Plugin installed at `~/.config/opencode/plugins/squeez.js`. OpenCode auto-loads plugins on startup. All Bash commands are automatically compressed via `squeez wrap`.

### GitHub Copilot CLI

Hooks registered in `~/.copilot/settings.json`. Session memory written to `~/.copilot/copilot-instructions.md` (Copilot CLI reads this automatically). State stored separately at `~/.copilot/squeez/`.

Refresh memory manually:

```bash
SQUEEZ_DIR=~/.copilot/squeez ~/.claude/squeez/bin/squeez init --copilot
```

---

## Local development

Requires Rust stable. Windows requires Git Bash.

```bash
git clone https://github.com/claudioemmanuel/squeez.git
cd squeez

cargo test                  # run all tests (356 tests, 37 suites)
cargo build --release       # build release binary

bash bench/run.sh           # filter-mode benchmark (14 fixtures)
bash bench/run_context.sh   # context-engine benchmark (3 wrap scenarios)
./target/release/squeez benchmark   # full 19-scenario benchmark suite

bash build.sh               # build + install to ~/.claude/squeez/bin/
```

---

## Contributing

```bash
git checkout -b feature/your-change
cargo test
cargo build --release
bash bench/run.sh
git push -u origin feature/your-change
gh pr create --base main --title "Short title" --body "Description"
```

CI runs `cargo test`, `bench/run.sh`, `bench/run_context.sh`, and `squeez benchmark` on every push and pull request.

See [CONTRIBUTING.md](CONTRIBUTING.md) for coding standards.

---

## License

Licensed under the **Apache License 2.0** — see [LICENSE](LICENSE) + [NOTICE](NOTICE).

Contributions require a DCO sign-off (`git commit -s …`) rather than a CLA. You keep copyright on what you contribute; sign-off is a lightweight affirmation that you have the right to submit it under Apache 2.0. See [CONTRIBUTING.md](CONTRIBUTING.md#license--contributor-sign-off) for details.
