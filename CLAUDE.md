# CLAUDE.md

Guidance for Claude Code working in this repo.

## Commands

```bash
cargo test                        # all unit tests; `cargo test <name>` for one
cargo build --release
bash build.sh                     # build + install to ~/.claude/squeez/bin/ + register hooks
                                  # NOTE: re-registers every host adapter, not just Claude Code
bash bench/run.sh                 # filter-mode benchmarks
python3 bench/verify_tokens.py    # real-tokenizer check (tiktoken cl100k_base) vs chars/4
./target/release/squeez benchmark [--json|--baseline|--efficiency-proof]
```

No Makefile — tooling is Cargo-native.

## Hard constraints

- **Zero runtime dependencies.** `Cargo.toml` lists only `libc` (Unix signal forwarding). Binary size and CI reproducibility depend on stdlib-only builds.
- **`squeez setup` must never shell out to an interpreter.** It used `python3 -c` until that broke setup on machines without `python3` on PATH, notably Windows (#190). Use `src/hosts/settings_json.rs` (pure Rust on `json_util`).
- **Every commit needs DCO sign-off** — CI enforces via `check-signoff`: `git commit -s -m "feat(scope): description"`.

## Architecture

Hook-based bash-output compressor for seven CLI agent hosts (Claude Code, Copilot CLI, OpenCode, Gemini CLI, Codex CLI, Pi, Hermes). Intercepts tool invocations and compresses output before the model sees it.

Claude Code hooks: PreToolUse → wrap / budget / prompt-compress / **agent-spawn ceiling**; SessionStart → init; PostToolUse → track-result + `updatedToolOutput` rewrite; SubagentStop → feed sub-agent output into SessionContext + release a spawn slot; PreCompact / PostCompact → log + re-arm.

**Pipeline** (per invocation): `smart_filter` (ANSI/progress/spinners/timestamps) → `dedup` (repeats → `[×N]`) → `grouping` (≥5 siblings → `dir/  N modified`) → `truncation` (head/tail by handler).

**Handler dispatch:** `src/filter.rs` detects command type → one of 13+ handlers in `src/commands/`. `extract_name()` strips wrappers (npx, bunx, pnpm exec, yarn exec) first. To add one: implement in `src/commands/`, register in `src/commands/mod.rs`, add a dispatch arm in `src/filter.rs`.

**Host adapters** (`src/hosts/`): one per CLI implementing `HostAdapter`. `squeez setup` iterates `all_hosts()` + `is_installed()`. `HostCaps` bitflags declare native support; Claude/Copilot/OpenCode get `BUDGET_HARD`, Gemini/Codex `BUDGET_SOFT`.

**Context engine** (`src/context/`) — cross-call awareness over 16 recent invocations:

| Module | Role |
|---|---|
| `cache.rs` | seen outputs/paths/errors; session-long skill + file-read dedup; `dedup_floor_call` fences pre-compact sources. **One global `context.json`, unlocked — concurrent sessions can clobber counters.** |
| `redundancy.rs` | exact FNV-1a hash, then fuzzy bottom-k MinHash trigram Jaccard ≥0.85 |
| `summarize.rs` | fires >500 lines (benign 2×); ≤40-line dense summary |
| `factsheet.rs` | exact-identifier extraction; 16 facts / 256 chars budget |
| `intensity.rs` | Full (×0.6) < `ultra_trigger_pct` of budget, else Ultra (×0.3). Budget honors `context_window_tokens` when pinned — **a pin above the real window makes Ultra unreachable** |
| `transcript.rs` | tail-reads host transcript for real `usage` → `real_ctx_tokens` |
| `hash.rs` | FNV-1a-64, `shingle_minhash()`, `jaccard()` |

Guards: identical image payloads dedup session-long; MCP results are dedup-only; fuzzy dedup suppressed on first re-read after a Write (needs compress-output before track-result in `posttooluse.sh`); repeated quota errors and oversized sub-agent returns queue `[squeez: …]` warnings.

**Known blind spots** (2026-08-21 post-mortem): agent cost is a flat `agent_spawn_cost` estimate regardless of real work; spawns are counted at PostToolUse (on return, not dispatch); `burst_warning()`/`agent_cost_warning()` are only called from `wrap.rs`, so they cannot surface unless a Bash command runs.

### Key files

| File | Role |
|---|---|
| `src/commands/wrap.rs` | orchestrator: spawn, capture, compress, inject header |
| `src/commands/compress_md/` | markdown compressor; `locale.rs` + `locales/{en,pt_br}.rs`; select via `lang=` / `--lang` |
| `src/commands/init.rs` | session start; delegates memory injection to `HostAdapter.inject_memory()` |
| `src/hosts/settings_json.rs` | shared JSON settings patcher: safe load, atomic write with `.bak`, idempotent hook add/strip |
| `src/commands/benchmark.rs` | 22-scenario suite; `--efficiency-proof` reports cache-aware effective costs |
| `src/config.rs` | config + `~/.claude/squeez/config.ini` parser; all fields defaulted |
| `src/tokens.rs` | zero-dep estimates: `estimate`, `estimate_scaled`, `classify` + `estimate_classed` |
| `src/session.rs` | token accounting; JSONL event log in `~/.claude/squeez/sessions/` |
| `src/commands/mcp_server.rs` | JSON-RPC 2.0 MCP server over stdio; 14 read-only tools |
| `src/commands/protocol.rs` | auto-teach payload; `full_payload()` returns ~2.4 KB |

### Tests

38+ integration files under `tests/` — one per strategy, handler, and host adapter. Notable: `test_redundancy_shingle.rs`, `test_mcp_server.rs`, `test_hosts_{registry,opencode,gemini,codex}.rs`. Benchmark fixtures in `bench/fixtures/` (capture: `bash bench/capture.sh`).

### Release

curl (local build), npm (pre-built download), cargo install. Release workflow builds universal macOS (lipo), Linux x86_64/aarch64 musl, Windows MSVC on tag push.
