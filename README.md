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

> **Windows users:** squeez requires **Git Bash** to run. PowerShell and CMD are not supported — the hooks and binary rely on a POSIX shell environment. Open Git Bash and run:

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

# Context engine (PR1)
adaptive_intensity = true           # auto-tighten compression as budget fills
context_cache_enabled = true        # persist cross-call state in sessions/context.json
redundancy_cache_enabled = true     # collapse identical recent outputs to a reference line
summarize_threshold_lines = 500     # raw lines above this trigger summary fallback
```

### Context engine

When `adaptive_intensity = true` (default), squeez compresses every bash call
at maximum aggression — limits ×0.3 across the board (Ultra mode):

- `max_lines` × 0.3 (floor 20)
- `dedup_min` × 0.5 (floor 2)
- `git_diff_max_lines`, `docker_logs_max_lines`, `find_max_results` × 0.3
- `summarize_threshold_lines` × 0.3 (floor 50)

Set `adaptive_intensity = false` to fall back to **Lite** (no scaling, raw
defaults). The active level is shown in the bash header:
`# squeez [git] 841→323 tokens (-62%) 55ms [adaptive: Ultra]`.

The `Lite` and `Full` enum variants remain for forward compatibility but are
not selected automatically — they exist so future versions can introduce
softer modes without breaking the public API.

When the same compressed output appears within the last 8 calls (length-equality
guarded), squeez replaces it with a single reference line:
`[squeez: identical to <hash> at bash#<n> — re-run with --no-squeez]`.

When raw output exceeds `summarize_threshold_lines`, squeez emits a dense
≤40-line summary (top errors, top files, test summary, last 20 lines verbatim)
instead of running the per-handler truncation pipeline.

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

**Prerequisites:** Rust stable, `bash` (Git Bash on Windows — PowerShell is not supported). Works on Windows (Git Bash), macOS, and Linux.

Install Rust via [rustup](https://rust-lang.org/tools/install/):
```bash
# macOS / Linux
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Windows: download and run rustup-init.exe from https://rust-lang.org/tools/install/
# Then restart your terminal so cargo is in PATH.
# Windows users also need MSVC C++ Build Tools:
# https://visualstudio.microsoft.com/visual-cpp-build-tools/
```

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

Please follow the repository branching and pull-request workflow to ensure stable releases and predictable merges.

Branching & PR rules

- Always branch from an up-to-date `develop` branch:
  - git fetch origin
  - git checkout develop
  - git pull
- Create a feature branch from `develop`:
  - git checkout -b feature/your-feature develop
- Implement your changes and run tests locally:
  - cargo test
  - cargo build --release
  - bash bench/run.sh  # optional for performance-sensitive changes
- Push your branch and create a PR targeting `develop`:
  - git push -u origin feature/your-feature
  - gh pr create --base develop --head feature/your-feature -t "Short title" -b "Description of changes and tests"
- Request reviewers, ensure CI passes and address feedback.
- Merge into `develop` once approved (follow the repo's merge strategy).

Promotion to main

- On push to `develop`, a workflow will create or update a PR from `develop` → `main` for final review and merge.
- Maintainers should review the `develop`→`main` PR, ensure CI/status checks pass, then merge to `main`.

Note about permissions

If the promotion workflow fails with an error like "GitHub Actions is not permitted to create or approve pull requests", you can resolve it by doing one of the following:

- Enable the repository setting: Settings → Actions → General → enable "Allow GitHub Actions to create and approve pull requests" (recommended).
- Or create a Personal Access Token (PAT) with `repo` scope and add it as a repository secret named `PR_CREATION_TOKEN`. The promotion workflow will use that secret to create/update the PR when present.

Create the secret via the GH CLI:

```bash
gh secret set PR_CREATION_TOKEN --body '<your-personal-access-token>'
```

Branch protection & admin notes

- Protect `main` with branch protection rules: require PR reviews (1+), require passing status checks, disallow force pushes, and enable required linear history if desired.
- If history was rewritten (commit messages edited), collaborators must re-sync their local clones:
  - git fetch origin
  - git reset --hard origin/develop
  - or re-clone the repository

Changelog (recent)

- 2026-04-06: Added `--codex` and `--antigravity` init scaffolding (scaffold-only; platform-specific integration may be required).
- 2026-04-06: Created `develop` branch and added a promotion workflow that creates/updates a PR from `develop` → `main` on push to `develop`.
- 2026-04-06: Fixed localization: removed Portuguese string in `install.sh` (now English).
- 2026-04-06: Removed `Co-authored-by: Copilot` trailers from commit messages (history rewritten).
- 2026-04-06: Deleted merged branches `iss-3/add-platforms-codex-antigravity`, `iss-5/fix-pt-strings`, and removed `feat/memory-subsystem` from the remote.

Benchmarks & running them

Benchmarks are in the `bench/` folder and can be run as follows:

```bash
cargo build --release
bash bench/run.sh
```

Reported results (macOS Apple Silicon; token estimate = chars/4) are shown in the "Benchmarks" table above. Re-run on your platform to verify and submit improvements as PRs.

See [CONTRIBUTING.md](CONTRIBUTING.md) for additional guidelines and coding standards.

