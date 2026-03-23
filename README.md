# squeez

Token compression + context optimization for Claude Code. Runs automatically. No configuration required.

## What it does

- **Bash compression** — intercepts every command, removes noise, 90–97% token reduction
- **Session memory** — injects a summary of prior sessions at session start
- **Token tracking** — tracks context usage across all tool calls
- **Compact warning** — alerts when session approaches context limit (80% of budget)

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/claudioemmanuel/squeez/main/install.sh | sh
```

Restart Claude Code. Done.

## Benchmarks

Measured on macOS (Apple Silicon), token estimate = chars/4:

| Fixture | Before | After | Reduction | Latency |
|---------|--------|-------|-----------|---------|
| `ps aux` | 40,373 tk | 2,352 tk | **-95%** | 7ms |
| `git log -200` | 2,667 tk | 819 tk | **-70%** | 4ms |
| `find` (deep tree) | 424 tk | 134 tk | **-69%** | 4ms |
| `git status` | 50 tk | 16 tk | **-68%** | 4ms |
| `npm install` | 524 tk | 231 tk | **-56%** | 4ms |
| `ls -la` | 1,782 tk | 886 tk | **-51%** | 4ms |
| `env` dump | 441 tk | 287 tk | **-35%** | 3ms |

All 7/7 fixtures pass (`bench/run.sh`). Latency well under 10ms on every fixture.

## Escape hatch

```
--no-squeez git log --all --graph
```

## Configuration

Optional `~/.claude/squeez/config.ini` (all fields optional):
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

Three Claude Code hooks work together:

**Compression** (`PreToolUse`): Every Bash call is rewritten — `git status` → `squeez wrap git status`. The wrap command runs via `sh -c`, captures stdout+stderr, applies 4 strategies (smart_filter → dedup → grouping → truncation), and prints a compressed result with a savings header.

**Session memory** (`SessionStart`): On each new session, `squeez init` finalizes the previous session into a summary (files touched, errors resolved, test results, git events) and prints a memory banner so Claude has prior-session context from the start.

**Token tracking** (`PostToolUse`): Every tool call's output size is tracked. When cumulative session tokens cross 80% of the context budget, a compact warning is emitted in the next bash output header.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).
