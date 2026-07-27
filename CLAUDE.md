# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working w/ code in this repo.

## Commands

```bash
cargo test                        # Run all unit tests
cargo test test_name              # Run a single test by name
cargo build --release             # Build optimized binary
bash build.sh                     # Build + install to ~/.claude/squeez/bin/ + register hooks

bash bench/run.sh                 # Filter-mode benchmarks (14 fixtures)
bash bench/run_context.sh         # Context engine benchmarks
python3 bench/verify_tokens.py    # Real-tokenizer (tiktoken cl100k_base) veracity check vs chars/4
./target/release/squeez benchmark # Full 19-scenario benchmark suite
./target/release/squeez benchmark --json  # JSON output
./target/release/squeez benchmark --efficiency-proof  # prove US-001/US-003/US-004 savings
```

No Makefile — all build tooling is Cargo-native.

## Contributing

Every commit must carry DCO sign-off — CI enforces this via `check-signoff` job:

```bash
git commit -s -m "feat(scope): description"
```

Or append manually to commit body:

```
Signed-off-by: Your Name <you@example.com>
```

## Architecture

**squeez** is hook-based bash output compressor for seven CLI agent hosts: Claude Code, Copilot CLI, OpenCode, Gemini CLI, Codex CLI, Pi, Hermes. It intercepts every tool invocation via host-specific hooks and runs output through 4-stage compression pipeline before model sees it. Claude Code hooks: PreToolUse → wrap/budget/prompt-compress, SessionStart → init, PostToolUse → track-result + updatedToolOutput rewrite (Read/Grep/Glob/Monitor), SubagentStop → feed sub-agent output into SessionContext, PreCompact → log event, PostCompact → re-arm reminder.

### Host adapters (`src/hosts/`)

One adapter per supported CLI, all implementing `HostAdapter` trait. `squeez setup` iterates registry via `all_hosts()` + `is_installed()` probes; `squeez init --host=<slug>` targets single host. Capability bitflags (`HostCaps::{BASH_WRAP, SESSION_MEM, BUDGET_HARD, BUDGET_SOFT}`) describe what each host supports natively — Claude/Copilot/OpenCode get BUDGET_HARD; Gemini/Codex ship BUDGET_SOFT (prose hints in `GEMINI.md` / `AGENTS.md`) pending upstream expansion.

### Compression pipeline (per command invocation)

1. **smart_filter** (`src/strategies/smart_filter.rs`) — strips ANSI codes, progress bars, spinners, timestamps
2. **dedup** (`src/strategies/dedup.rs`) — collapses repeated lines to `[×N]` annotations
3. **grouping** (`src/strategies/grouping.rs`) — collapses sibling files in same dir (≥5) to `dir/  N modified`
4. **truncation** (`src/strategies/truncation.rs`) — head/tail depending on handler type

### Handler dispatch

`src/filter.rs` detects command type and routes to one of 13+ handlers in `src/commands/`. `extract_name()` strips wrappers (npx, bunx, pnpm exec, yarn exec) before matching. Adding new handler: implement handler in `src/commands/`register in `src/commands/mod.rs`add dispatch arm in `src/filter.rs`.

### Context engine (`src/context/`)

Cross-call awareness across 16 recent invocations:
- **cache.rs** — tracks seen outputs, file paths, errors from Read/Glob/Grep/Bash results. Two session-long stores beyond the 16-call window: skill bodies (`skill_dedup_*`) and file reads (`read_dedup_*`, keyed by path + content hash, invalidated on Write/Create). `dedup_floor_call` fences off every dedup source recorded before a session change or a `/compact` — `context.json` outlives both, so without it squeez cites content the model no longer has
- **redundancy.rs** — two-path dedup: exact FNV-1a hash (fast), then fuzzy bottom-k MinHash trigram Jaccard ≥0.85 (whitespace/timestamp changes don't break match). Emits `[squeez: identical to ...]`  `[squeez: ~P% similar to ...]`
- **summarize.rs** — triggered at >500 lines; benign outputs (no error markers) get 2× threshold (1000 lines). Produces ≤40-line dense summary (errors, files, test status, verbatim tail)
- **factsheet.rs** — deterministic extraction of exact identifiers (hex/SHA ≥7 w/ digit, UUID, ticket code, version, int ≥6 digits) from region summarize drops; budget 16 facts / 256 chars, substring-collapse, tier-then-length ordering; >MAX_FACTS distinct versions = generated-sequence noise, tier dropped. Rides in summaries as `ids_preserved:` / `"ids":[...]`
- **intensity.rs** — truly adaptive: **Full** (×0.6) when used < 65% of budget, **Ultra** (×0.3) when ≥65% (`ultra_trigger_pct`default 0.65). `[adaptive: Full]`  `[adaptive: Ultra]` in header. Budget honors `context_window_tokens` when set; "used" is `max(squeez counters, real_ctx_tokens)`
- **transcript.rs** — real-context tracking: tail-reads host transcript (`transcript_path` from PostToolUse payload) and extracts last assistant turn's `usage` (input + cache_read + cache_creation) → `SessionContext.real_ctx_tokens`. Lets intensity/burn-rate fire in MCP/image-heavy sessions whose tokens never pass through squeez
- **hash.rs** — FNV-1a-64 + `shingle_minhash()` (bottom-k=96, whitespace-token trigrams) + `jaccard()` (sorted-merge O(n+m))

Guards: byte-identical image payloads dedup session-long (`image_fp`); MCP results are dedup-only (never summarized); fuzzy dedup is suppressed for first re-read of file flagged Write since last seen (A2 stale-state guard — requires compress-output to run before track-result in posttooluse.sh); repeated quota/plan-limit errors (≥`quota_error_threshold`default 2) and oversized sub-agent returns (>`subagent_result_warn_tokens`default 3000) queue `[squeez: …]` warnings drained by next `squeez wrap`.

### Key files

| File | Role |
|------|------|
| `src/commands/wrap.rs` | Main orchestrator: spawn subprocess, capture, compress, inject header |
| `src/commands/compress_md/` | Markdown compressor module: `mod.rs` (core logic), `locale.rs` (Locale struct + `from_code`), `locales/en.rs` + `locales/pt_br.rs` (word lists). Exposes `compress_text` (EN default) and `compress_text_with_locale`. Select locale via `lang=` in config or `--lang` CLI flag. |
| `src/commands/init.rs` | Session start orchestrator: finalizes previous session, prints banner, delegates memory injection to `HostAdapter.inject_memory()` via `run_for_host(slug)` |
| `src/hosts/` | Per-host adapters (`claude_code.rs` / `copilot.rs` / `opencode.rs` / `gemini.rs` / `codex.rs` / `pi.rs` / `hermes.rs`) implementing the `HostAdapter` trait + `HostCaps` bitflags + `all_hosts()` / `find()` registry. Each adapter owns install/uninstall + memory-file injection for its CLI. |
| `src/hosts/settings_json.rs` | Shared JSON settings patcher used by every adapter that registers hooks: safe load (refuses malformed / non-object files), atomic write with `.bak`, and idempotent hook add/strip helpers. Pure Rust on top of `json_util` — **`squeez setup` must never shell out to an interpreter**; it did (`python3 -c`) until that broke setup on any machine without `python3` on PATH, notably Windows (#190). |
| `src/commands/setup.rs` | `squeez setup` — thin orchestrator over the adapter registry |
| `src/commands/uninstall.rs` | `squeez uninstall` — reverse registration, preserves session data |
| `src/commands/benchmark.rs` | 22-scenario reproducible benchmark suite (incl. 3 economy scenarios); `--baseline` flag prints C0 vs C4 A/B table; `--efficiency-proof` reports cache-aware effective costs (ratios ×1.0/×1.25/×0.1/×5 applied to both sides under same cache state, negatives unfloored, JSON `schema_version:2`) |
| `src/config.rs` | Config struct + `~/.claude/squeez/config.ini` parser; all fields have defaults. Key tunables: `focus` (default `off`) — ADHD output-structure axis, orthogonal to `persona`; drives `src/commands/focus.rs` + `assets/focus_adhd*.md`, the banner order in `init.rs::banner_body`, the advisory cap in `wrap.rs`, and doctor ordering; `state_warn_calls` (default 10) — calls-remaining threshold for State-First Pattern warning; `class_density` (default true) — content-class token estimate in wrap; `net_win_min_tokens` (default 24) — passthrough gate when compression saves less than header cost |
| `src/tokens.rs` | Zero-dep token estimate: char-class base (`estimate`), model-density scale (`estimate_scaled`), content-class calibrated (`classify` Dense/Prose/Mixed + `estimate_classed`: chars/2.0 dense, chars/3.7 prose, base for mixed) |
| `src/context/factsheet.rs` | Exact-identifier extraction for summaries (see Context engine) — zero-dep hand-rolled scanners, no regex |
| `src/session.rs` | Session state: token accounting, JSONL event log at `~/.claude/squeez/sessions/` |
| `src/context/cache.rs` | Cross-call dedup + file access cache (16-call window); stores shingles for fuzzy match |
| `src/commands/mcp_server.rs` | JSON-RPC 2.0 MCP server over stdio; 14 read-only tools (`squeez_recent_calls`, `squeez_seen_files`, `squeez_seen_errors`, `squeez_seen_error_details`, `squeez_session_summary`, `squeez_session_detail`, `squeez_session_stats`, `squeez_session_efficiency`, `squeez_prior_summaries`, `squeez_search_history`, `squeez_file_history`, `squeez_agent_costs`, `squeez_protocol`, `squeez_context_pressure`) |
| `src/commands/protocol.rs` | Auto-teach payload — `SQUEEZ_PROTOCOL` + `SQUEEZ_MARKERS_SPEC` constants; `full_payload()` returns combined 2.4 KB string |

### Zero-dependency constraint

`Cargo.toml` lists only `libc` (Unix signal forwarding). Do not add runtime dependencies — binary size and CI reproducibility depend on stdlib-only builds.

### Tests

38+ integration test files under `tests/`. Each strategy, handler, and host adapter has dedicated test file. Notable: `test_redundancy_shingle.rs` (fuzzy-match), `test_mcp_server.rs` (JSON-RPC), `test_hosts_{registry,opencode,gemini,codex}.rs` (adapter install/uninstall). Benchmark fixtures live in `bench/fixtures/`capture new ones w/ `bash bench/capture.sh`.

### Release & distribution

Three install methods: curl (local build), npm (pre-built binary download), cargo install. Release workflow builds universal macOS (lipo arm64+x86_64), Linux x86_64/aarch64 musl, Windows MSVC — all via GitHub Actions on tag push.

