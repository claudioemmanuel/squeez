# squeez

[![CI](https://github.com/claudioemmanuel/squeez/actions/workflows/ci.yml/badge.svg)](https://github.com/claudioemmanuel/squeez/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org)

Token compression + context optimization for Claude Code, OpenCode, and GitHub Copilot CLI. Runs automatically in Claude Code and Copilot CLI. Manual usage in OpenCode.

## What it does

- **Bash compression** — intercepts every command, removes noise, up to 95% token reduction
- **Session memory** — injects a summary of prior sessions at session start
- **Token tracking** — tracks context usage across all tool calls
- **Compact warning** — alerts when session approaches context limit (80% of budget)

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/claudioemmanuel/squeez/main/install.sh | sh
```

- **Claude Code:** Restart Claude Code to activate
- **OpenCode:** Restart OpenCode to activate the plugin
- **Copilot CLI:** Memory injected into `~/.copilot/copilot-instructions.md`; restart Copilot CLI to activate hook-based bash compression

## Benchmarks

Measured on macOS (Apple Silicon), token estimate = chars/4. Run with `bash bench/run.sh`.

| Fixture | Before | After | Reduction | Latency |
|---------|--------|-------|-----------|---------|
| `ps aux` | 40,373 tk | 2,352 tk | **-95%** | 6ms |
| `git log` (200 commits) | 2,667 tk | 819 tk | **-70%** | 4ms |
| `docker logs` | 665 tk | 186 tk | **-73%** | 5ms |
| `find` (deep tree) | 424 tk | 134 tk | **-69%** | 3ms |
| `git status` | 50 tk | 16 tk | **-68%** | 3ms |
| `ls -la` | 1,782 tk | 886 tk | **-51%** | 4ms |
| `npm install` | 524 tk | 231 tk | **-56%** | 3ms |
| `git diff` | 502 tk | 317 tk | **-37%** | 4ms |
| `env` dump | 441 tk | 287 tk | **-35%** | 3ms |
| Copilot CLI session | 639 tk | 421 tk | **-35%** | 3ms |

10/10 fixtures pass. Latency under 10ms on every fixture.

## Escape hatch

```
--no-squeez git log --all --graph
```

## Configuration

Optional config file (all fields optional):
- Claude Code / default: `~/.claude/squeez/config.ini`
- Copilot CLI: `~/.copilot/squeez/config.ini`
```ini
# Compression
max_lines = 200
dedup_min = 3
git_log_max_commits = 20
docker_logs_max_lines = 100
bypass = docker exec, psql, ssh

# Session memory
compact_threshold_tokens = 160000   # warn at 80% of context budget
memory_retention_days = 30          # how long to keep session summaries
```

## How it works

### Claude Code & Copilot CLI

Three hooks work together:

**Compression** (`PreToolUse`): Every Bash call is rewritten — `git status` → `squeez wrap git status`. The wrap command runs via `sh -c`, captures stdout+stderr, applies 4 strategies (smart_filter → dedup → grouping → truncation), and prints a compressed result with a savings header.

**Session memory** (`SessionStart`): On each new session, `squeez init` finalizes the previous session into a summary (files touched, errors resolved, test results, git events) and prints a memory banner so the agent has prior-session context from the start. For Copilot CLI, this banner is also written to `~/.copilot/copilot-instructions.md` which is loaded automatically at every session.

**Token tracking** (`PostToolUse`): Every tool call's output size is tracked. When cumulative session tokens cross 80% of the context budget, a compact warning is emitted in the next bash output header.

## OpenCode

OpenCode is supported via an auto-loading plugin that intercepts all Bash commands.

**Install:**
```bash
curl -fsSL https://raw.githubusercontent.com/claudioemmanuel/squeez/main/install.sh | sh
```

**What happens:**
- Plugin is installed to `~/.config/opencode/plugins/squeez.js`
- OpenCode auto-loads plugins on startup
- All Bash commands are automatically compressed via `squeez wrap`

**Escape hatch:**
```bash
--no-squeez git log --all --graph
```

**Manual usage:**
```bash
squeez wrap git status
squeez wrap docker logs mycontainer
squeez wrap npm install
```

## GitHub Copilot CLI

Copilot CLI is supported via hooks registered in `~/.copilot/settings.json` and session memory injected into `~/.copilot/copilot-instructions.md`.

**Install:**
```bash
curl -fsSL https://raw.githubusercontent.com/claudioemmanuel/squeez/main/install.sh | sh
```

**What happens:**
- Session memory is written to `~/.copilot/copilot-instructions.md` — Copilot CLI reads this automatically at every session start, giving the agent prior-session context without re-discovery tokens
- Hooks are registered in `~/.copilot/settings.json` for PreToolUse (bash compression), SessionStart (memory refresh), and PostToolUse (token tracking)
- Session state is stored separately in `~/.copilot/squeez/` (independent from Claude Code state)

**Refresh memory manually:**
```bash
SQUEEZ_DIR=~/.copilot/squeez ~/.claude/squeez/bin/squeez init --copilot
```

**Escape hatch:**
```bash
--no-squeez git log --all --graph
```

## Local development

**Prerequisites:** Rust stable (`rustup update stable`), `bash`, macOS or Linux.

```bash
# 1. Clone
git clone https://github.com/claudioemmanuel/squeez.git
cd squeez

# 2. Build & test
cargo test

# 3. Run benchmarks (uses local release binary automatically)
cargo build --release
bash bench/run.sh

# 4. Install hooks into Claude Code and Copilot CLI config
bash install.sh

# 5. Restart Claude Code / Copilot CLI — squeez is active
```

To uninstall: `bash uninstall.sh`

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).
