# squeez

Token compression + context optimization for Claude Code. Runs automatically. No configuration required.

## What it does

Intercepts every Bash command Claude runs, compresses noisy output, and returns a token-efficient summary. 90–97% reduction on the most verbose commands.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/claudioemmanuel/squeez/main/install.sh | sh
```

Restart Claude Code. Done.

## Benchmarks

| Command | Before | After | Reduction |
|---------|--------|-------|-----------|
| `git log -200` | ~3,200 tk | ~190 tk | **-94%** |
| `docker logs (noisy)` | ~8,200 tk | ~620 tk | **-92%** |
| `npm install` | ~6,100 tk | ~180 tk | **-97%** |
| `gradle build` | ~18,000 tk | ~400 tk | **-98%** |
| GraphQL error | ~850 tk | ~80 tk | **-91%** |

*Token estimate: chars/4*

## Escape hatch

```
--no-squeez git log --all --graph
```

## Configuration

Optional `~/.claude/squeez/config.ini` (all fields optional):
```ini
max_lines = 200
dedup_min = 3
git_log_max_commits = 20
docker_logs_max_lines = 100
bypass = docker exec, psql, ssh
```

## How it works

A Claude Code `PreToolUse` hook rewrites every Bash tool call:
`git status` → `squeez wrap git status`

`squeez wrap` runs the command via `sh -c`, captures stdout+stderr, applies 4 strategies (smart_filter → dedup → grouping → truncation), prints the compressed result with a savings header.

Claude never sees raw noisy output.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).
