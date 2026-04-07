# squeez

[![CI](https://github.com/claudioemmanuel/squeez/actions/workflows/ci.yml/badge.svg)](https://github.com/claudioemmanuel/squeez/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/squeez.svg)](https://www.npmjs.com/package/squeez)
[![Crates.io](https://img.shields.io/crates/v/squeez.svg)](https://crates.io/crates/squeez)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

End-to-end token optimizer for Claude Code, OpenCode, and GitHub Copilot CLI. Compresses bash output up to **95%**, collapses redundant calls, and injects a terse prompt persona — automatically, with zero new runtime dependencies.

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

### After install

| Platform | What to do |
|----------|-----------|
| **Claude Code** | Restart Claude Code — hooks activate automatically |
| **OpenCode** | Restart OpenCode — plugin auto-loads from `~/.config/opencode/plugins/` |
| **Copilot CLI** | Restart Copilot CLI — hooks registered in `~/.copilot/settings.json` |

### Uninstall

```bash
bash ~/.claude/squeez/uninstall.sh
# or, if you cloned the repo:
bash uninstall.sh
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
| **MCP server** | `squeez mcp` runs a JSON-RPC 2.0 server over stdio exposing 6 read-only tools so any MCP-compatible LLM can query session memory directly. Hand-rolled, no `mcp.server` dependency. |
| **Auto-teach payload** | `squeez protocol` (or the `squeez_protocol` MCP tool) prints a 2.4 KB self-describing payload — the LLM learns squeez's markers and protocol on first call. |
| **Caveman persona** | Injects an ultra-terse prompt at session start so the model responds with fewer tokens. |
| **Memory-file compression** | `squeez compress-md` compresses CLAUDE.md / AGENTS.md / copilot-instructions.md in-place — pure Rust, zero LLM. i18n-aware: set `lang = pt` (or `--lang pt`) for pt-BR article/filler/phrase dropping and Unicode-correct matching. |
| **Session memory** | On `SessionStart`, injects a summary of the previous session (files touched, errors, test results, git events). Summaries carry temporal validity (`valid_from`/`valid_to`) so invalidated entries age from `valid_to`. |
| **Token tracking** | Every `PostToolUse` result (Bash, Read, Grep, Glob) feeds a `SessionContext` so squeez knows what the agent has already seen. |

---

## Benchmarks

Measured on macOS (Apple Silicon). Token count = `chars / 4` (matches Claude's ~4 chars/token). Run `squeez benchmark` to reproduce.

### Per-scenario results — 19 scenarios × 3 iterations

| Scenario | Before | After | Reduction | Latency |
|----------|--------|-------|-----------|---------|
| `ps aux` (161 KB real output) | 40,373 tk | 2,352 tk | **−94%** | 1.8 ms |
| 5,003-line log (summarize path) | 82,257 tk | 420 tk | **−99.5%** | 63 ms |
| Repetitive output (300× dedup) | 4,692 tk | 37 tk | **−99.2%** | 0.2 ms |
| `git log` (200 commits) | 2,692 tk | 289 tk | **−89%** | 0.2 ms |
| `tsc` errors | 731 tk | 101 tk | **−86%** | 0.06 ms |
| `cargo build` (noisy + errors) | 2,106 tk | 452 tk | **−79%** | 0.2 ms |
| `docker logs` | 665 tk | 186 tk | **−72%** | 0.05 ms |
| `find` (deep tree) | 424 tk | 134 tk | **−68%** | 0.07 ms |
| `git status` | 50 tk | 16 tk | **−68%** | 0.02 ms |
| Verbose app log (250 lines) | 4,957 tk | 1,991 tk | **−60%** | 0.3 ms |
| `npm install` | 524 tk | 232 tk | **−56%** | 0.04 ms |
| Cross-call redundancy (3× same) | 486 tk | 241 tk | **−50%** | 58 ms |
| `ls -la` | 1,782 tk | 886 tk | **−50%** | 0.1 ms |
| `env` dump | 441 tk | 287 tk | **−35%** | 0.03 ms |
| `git diff` | 502 tk | 497 tk | **−1%** | 0.05 ms |
| CLAUDE.md (compress-md) | 316 tk | 247 tk | **−22%** | 0.2 ms |

### Aggregate

| Metric | Value |
|--------|-------|
| **Total token reduction** | **92.8%** — 145,338 tk → 10,441 tk |
| Bash output | **−84.9%** |
| Markdown / context files | **−23.3%** |
| Wrap / cross-call engine | **−99.2%** |
| Quality (signal terms preserved) | **19 / 19 pass** |
| Latency p50 (filter mode) | **< 0.3 ms** |
| Latency p95 (incl. wrap/summarize) | **64 ms** |

### compress-md i18n — EN vs pt-BR (Apple Silicon, release build)

| Locale | Mode | Before | After | Reduction | Latency |
|--------|------|--------|-------|-----------|---------|
| EN | Full | 514 tk | 445 tk | **−14%** | 170 µs |
| EN | Ultra | 514 tk | 434 tk | **−16%** | — |
| pt-BR | Full | 558 tk | 488 tk | **−13%** | 256 µs |
| pt-BR | Ultra | 558 tk | 468 tk | **−17%** | — |

PT-BR is **~1.5× slower** than EN due to Unicode case folding — still sub-millisecond per call. Both locales produce `result.safe = true`. Run `cargo run --release --bin bench_i18n` to reproduce.

**Before / after — pt-BR Full mode:**
```
IN:    O sistema é basicamente apenas uma ferramenta para configurar o repositório.
       De modo geral, você pode considerar que a função principal inicializa a documentação do projeto.

Full:  sistema é ferramenta para configurar repositório. função principal inicializa documentação projeto.
Ultra: sistema é ferramenta p/ configurar repo. fn principal inicializa docs projeto.
```

Drops: articles (`o`, `a`, `do`), fillers (`basicamente`, `apenas`), phrases (`De modo geral`, `você pode considerar que`). Ultra adds abbreviations (`repositório→repo`, `função→fn`, `documentação→docs`, `para→p/`).

### Estimated cost savings — Claude Sonnet 4.6 · $3.00 / MTok input

| Usage | Baseline / month | Saved / month |
|-------|-----------------|---------------|
| 100 calls / day | $18.00 | **$16.71 (93%)** |
| 1,000 calls / day | $180.00 | **$167.07 (93%)** |
| 10,000 calls / day | $1,800.00 | **$1,670.69 (93%)** |

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

Six read-only tools become available to the LLM:

| Tool | Returns |
|------|---------|
| `squeez_recent_calls` | Last N bash invocations with hash + length + cmd snippet — check before re-running |
| `squeez_seen_files` | Files this session has touched (Read tool + paths extracted from bash output), sorted by recency |
| `squeez_seen_errors` | Distinct error fingerprints observed this session (FNV-1a hashes of normalized errors) |
| `squeez_session_summary` | Token accounting + call counts (tokens_bash / tokens_read / tokens_other / seen_files / seen_errors / seen_git_refs) |
| `squeez_prior_summaries` | Last N finalized prior-session summaries from `~/.claude/squeez/memory/summaries.jsonl` |
| `squeez_protocol` | Auto-teach payload — read once per session to learn squeez's markers + memory protocol |

All read-only. All backed by `SessionContext::load()` and `memory::read_last_n()`. No side effects.

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
| Build systems | `make`, `cmake`, `gradle`, `mvn`, `xcodebuild`, `cargo` (build) |
| Test runners | `cargo test`, `jest`, `vitest`, `pytest`, `nextest` |
| TypeScript / linters | `tsc`, `eslint`, `biome` |
| Cloud CLIs | `kubectl`, `gh`, `aws`, `gcloud`, `az` |
| Databases | `psql`, `prisma`, `mysql` |
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

cargo test                  # run all tests (315 tests, 38 suites)
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

MIT — see [LICENSE](LICENSE).
