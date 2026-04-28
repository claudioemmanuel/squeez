# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Future entries are maintained automatically by release-please from
conventional commit messages on `main`.

## [Unreleased]

## [1.11.0] - 2026-04-28

### Added
- session-analysis improvements — az JSON, vite noise, prisma generate, hook health check (#90)
- feat(fs): auto-compress cat'd markdown via compress_md pipeline (#88)
- feat(economy): compress Agent/Task prompt at PreToolUse time (#87)
- feat(handlers): xcodebuild noise filter + log-file tail detection (#84)
- feat(bench): efficiency-proof benchmark suite — quantitative savings for US-001/003/004 (#74)
- token-compression power-up — sig-mode, memory-file warn, structured summaries, hypothesis benchmark (#70)
- feat(setup): adapter-driven setup/uninstall across all hosts (#58)
- feat(hosts): Codex CLI adapter (hooks + AGENTS.md soft-budget) (#55)
- feat(hosts): Gemini CLI adapter (BeforeTool/AfterTool hooks) (#52)
- feat(hosts): full-parity OpenCode adapter + plugin rewrite (#48)
- wrangler, playwright, next build handlers + bun/drizzle routing (#34)
- feat(opus47): adapt squeez for Opus 4.7 tokenizer and xhigh effort
- feat(setup): auto-detect lang from CLAUDE.md/locale + stderr update notification (#29)
- v0.3.0 — token economy, pt-BR caveman, aggressive defaults, full hook install
- structured session memory, cross-session search, error snippets, configurable tunables
- feat(setup): create config.ini on first install
- feat(i18n): locale-aware compress-md with pt-BR support (#25)
- fuzzy dedup, adaptive intensity, benign summarize, MCP server, temporal memory, protocol payload
- Windows setup via cargo/npm + squeez update fix
- feat(benchmark): add squeez benchmark command with 19-scenario test suite
- feat(ci): re-add promote-develop workflow with correct registration
- feat(init): inject persona into ~/.claude/CLAUDE.md on every session start
- feat(npm+crates): add npm package and publish jobs to release workflow
- feat(statusline): add statusline.sh to repo; install via install.sh; show all-time stats
- compress-md, caveman persona, squeez update, track-result hook
- feat(context): adaptive intensity always picks Ultra
- feat(context): aggressive token-engine — adaptive intensity, redundancy cache, summarize fallback
- add Windows support to install.sh and release.yml
- add home_dir() helper with USERPROFILE fallback for Windows
- cross-platform wrap — shell_command() helper, cfg(unix) signal gates, libc unix-only
- add GitHub Copilot CLI support
- add OpenCode plugin for automatic Bash compression
- add universal installer support for macOS and Linux
- expand benchmarks, edge-case tests, and README polish
- add SessionStart + PostToolUse hooks, update install.sh and README
- wrap.rs — artifact capture (files/errors/git/tests) + compact warning header
- squeez init — session start, prior memory banner, session finalization (TDD)
- squeez track — token accumulation + session event log (TDD)
- memory.rs — Summary, read_last_n, write_summary, prune_old (TDD)
- session.rs — CurrentSession, append_event, date math (TDD)
- json_util helpers + compact_threshold_tokens, memory_retention_days config
- benchmark system — all fixtures passing
- build.sh compiles + installs squeez, registers hook
- PreToolUse hook script
- fs and runtime handlers — all handler tests passing
- cloud, database, network handlers (TDD)
- typescript and build handlers (TDD)
- package_mgr and test_runner handlers (TDD)
- git and docker handlers (TDD)
- wrap subcommand — sh -c, 120s timeout, signal forwarding, bypass check
- Handler trait with cmd+config, generic handler, command router, filter_stdin
- truncation strategy (TDD) — all strategy tests passing
- grouping strategy (TDD)
- dedup strategy (TDD)
- smart_filter strategy with node_modules frame removal (TDD)
- implement config.ini parser (TDD)

### Changed
- chore(release): bump version to 1.11.0 [squeez-release-bot]
- docs: update changelog and benchmarks for v1.10.0
- docs: update changelog and benchmarks for v1.9.0
- chore(release): bump version to 1.10.0 [squeez-release-bot]
- docs(readme): document squeez's compression scope and architectural limits (#85)
- chore(release): bump version to 1.9.0 [squeez-release-bot]
- chore(release): bump version to 1.8.0 [squeez-release-bot]
- docs: update changelog and benchmarks for v1.7.7
- chore(release): bump version to 1.7.7 [squeez-release-bot]
- docs: update changelog and benchmarks for v1.7.6
- chore(release): bump version to 1.7.6 [squeez-release-bot]
- docs: update changelog and benchmarks for v1.7.5
- chore(release): bump version to 1.7.5 [squeez-release-bot]
- docs: update changelog and benchmarks for v1.7.4
- chore(release): bump version to 1.7.4 [squeez-release-bot]
- perf: O(log n) refactor — tail read, offset index, single-pass parser
- docs: update changelog and benchmarks for v1.7.3
- chore(release): bump version to 1.7.3 [squeez-release-bot]
- chore(release): bump version to 1.7.2 [squeez-release-bot]
- restore: revert accidental deletion of auto-release.yml workflow
- docs: update changelog and benchmarks for v1.7.1
- chore(release): bump version to 1.7.1 [squeez-release-bot]
- chore: relicense to Apache 2.0 + add DCO policy + banner (#76)
- docs: update changelog and benchmarks for v1.7.0
- chore(release): bump version to 1.7.0 [squeez-release-bot]
- docs: update changelog and benchmarks for v1.6.1
- chore(release): bump version to 1.6.1 [squeez-release-bot]
- docs: update changelog and benchmarks for v1.6.0
- chore(release): bump version to 1.6.0 [squeez-release-bot]
- docs: update changelog and benchmarks for v1.5.2
- chore(release): bump version to 1.5.2 [squeez-release-bot]
- chore(release): bump version to 1.5.1 [squeez-release-bot]
- docs: five-host matrix + squeez setup/uninstall flow (#63)
- refactor(install): delegate host registration to 'squeez setup' (#60)
- chore(release): bump version to 1.5.0 [squeez-release-bot]
- chore(release): bump version to 1.4.0 [squeez-release-bot]
- chore(release): bump version to 1.3.0 [squeez-release-bot]
- chore(release): bump version to 1.2.0 [squeez-release-bot]
- chore(release): bump version to 1.1.0 [squeez-release-bot]
- chore(release): bump version to 1.0.0 [skip release]
- refactor(hosts): extract HostAdapter trait + migrate Claude Code and Copilot (#42)
- ci: conventional-commit-driven auto-release (#40)
- chore(gov): issue-form templates + refined PR template (#39)
- docs: update changelog and benchmarks for v0.4.1 [skip release] (#33)
- chore(release): bump version to 0.4.1
- chore(release): bump version to 0.4.0
- docs: update changelog and benchmarks for v0.3.2 [skip release] (#30)
- chore: bump version to 0.3.2
- docs: add shields.io badge for changelog link
- chore: bump version to 0.3.1
- chore: bump version to 0.2.3
- docs: update README with 11 MCP tools, structured memory, and advanced tunables
- chore: bump version to 0.2.2
- docs: rewrite README with full install instructions (curl/npm/cargo) and all commands
- chore: refresh bench/report.md from latest run [skip release]
- chore(ci): remove promote-develop workflow (re-adding to fix broken registration)
- chore: bump version to 0.2.1 + auto-release on merge to main [skip release]
- chore: bump version to 0.2.0; add Cargo.toml metadata
- ci: PR template, linked-issue enforcement, no-claude-trailer check
- docs: update README with PR1+PR2 features, fresh benchmarks, changelog
- ci: auto-delete branches after PR merge
- docs: update Windows installation instructions and prerequisites in README
- docs: add Windows support and Rust install instructions to README
- refactor: use session::home_dir() in config and init (Windows compat)
- ci: trigger promote workflow (test)
- ci(workflow): avoid failing promote job when PR creation is not permitted; add PR_CREATION_TOKEN guidance
- docs(readme): add changelog, contribution workflow, and benchmark notes
- ci(workflow): create/update PR from develop to main on push
- chore: add .worktrees/ to gitignore
- chore: add docs/ to gitignore (local design specs)
- docs: update README for GitHub Copilot CLI support
- security: fix remaining LOW findings in wrap.rs
- security: fix 2 follow-up findings from second audit
- security: fix all 8 findings from security audit
- ci: build and install binary before running benchmarks
- chore: add GitHub sponsor button
- chore: ignore .omc/ directory, remove from tracking
- chore: update benchmark report (7/7 passing)
- docs: update README — real benchmarks, Phase 2 config, how it works
- style: cargo fmt on session.rs and test_session.rs
- chore: open source files — README, LICENSE, CI/release workflows, install.sh
- chore: scaffold squeez Rust project

### Fixed
- fix(hosts): expand PreToolUse matcher to cover Read, Grep, Glob, Agent, Task (#86)
- fix(json_util,statusline): whitespace-tolerant JSON parser + UTF-8 statusline on Windows (#81)
- fix(hosts): preserve user settings.json when existing JSON fails to parse (#83)
- prevent OOM from unbounded file reads + data integrity fixes
- fix(ci): use native ubuntu-24.04-arm runner for aarch64 build
- eliminate memory leaks causing 16 GB RAM on MCP server startup (#79)
- fix(hooks): add hookEventName to PreToolUse hookSpecificOutput (#78)
- fix(opencode): align plugin with PluginModule SDK shape (default export + server map) (#72)
- fix(ci): push release docs directly to main (loop-safe) (#68)
- back Copilot BUDGET_HARD with Read/Grep hook + update upstream refs (#65)
- fix(ci): startsWith guard for auto-release recursion (#46)
- fix(ci): tighten auto-release bump detection + fix tag push (#44)
- fix(ci): add --allow-dirty to cargo publish for Cargo.lock changes
- fix(ci): skip publish gracefully when version already exists on crates.io/npm
- fix(ci): run update-docs even if publish jobs fail (already published)
- fix(ci): continue-on-error for publish jobs so update-docs always runs
- fix(ci): add pull-requests write permission to update-docs job
- fix(update): cargo-managed self-update + Windows deferred install + session update check (#27)
- fix(ci): update-docs creates PR instead of pushing directly to protected main
- fix(npm): copy README.md into npm package before publish; add readme field
- fix(ci): add toolchain: stable to all dtolnay/rust-toolchain steps
- fix(ci): handle no previous tags in generate-changelog.sh (pipefail + grep -v)
- fix(ci): dispatch release.yml from auto-release to bypass GITHUB_TOKEN chain limit [skip release]
- fix(ci): fix flaky test tmp dir collision + simplify auto-release
- fix(bench): exclude synthetic fixtures from filter-mode bench/run.sh
- fix(ci): remove secrets in step if-conditions — check token inside script instead
- fix(ci): rename promote-develop → promote-to-main to fix broken workflow registration
- fix(ci): bump github-script to v7 in promote-develop to force workflow re-registration
- fix(compress_md): use byte slice comparison to avoid UTF-8 char boundary panic
- fix(statusline): remove Ctx% — show only squeez compression metrics
- fix(statusline): green ✓, proper capitalization, real-time context %, all metrics
- resolve gap analysis — new handlers, wider redundancy window, token budget
- install.sh — clarify PostToolUse no-matcher intent, guard version echo
- wrap.rs — safe git hex check, O(n) dedup, full cmd in event log
- use ends_with for files_committed check in init finalize
- json_util — guard extract_u64 empty digit, add json_util test coverage
- add pre-flight binary check to bench/run.sh
- atomic settings.json write + uninstall safety guards
- harden pretooluse hook (shlex.quote, error handling, single path source)
- wrap — process_group(0) for correct signal forwarding, tighten is_streaming

## [1.10.0] - 2026-04-22

### Added
- feat(fs): auto-compress cat'd markdown via compress_md pipeline (#88)
- feat(economy): compress Agent/Task prompt at PreToolUse time (#87)
- feat(handlers): xcodebuild noise filter + log-file tail detection (#84)

### Changed
- chore(release): bump version to 1.10.0 [squeez-release-bot]
- docs(readme): document squeez's compression scope and architectural limits (#85)
- chore(release): bump version to 1.9.0 [squeez-release-bot]
- chore(release): bump version to 1.8.0 [squeez-release-bot]
- docs: update changelog and benchmarks for v1.7.7
- chore(release): bump version to 1.7.7 [squeez-release-bot]
- docs: update changelog and benchmarks for v1.7.6
- chore(release): bump version to 1.7.6 [squeez-release-bot]
- docs: update changelog and benchmarks for v1.7.5
- chore(release): bump version to 1.7.5 [squeez-release-bot]
- docs: update changelog and benchmarks for v1.7.4
- chore(release): bump version to 1.7.4 [squeez-release-bot]
- perf: O(log n) refactor — tail read, offset index, single-pass parser
- docs: update changelog and benchmarks for v1.7.3
- chore(release): bump version to 1.7.3 [squeez-release-bot]

### Fixed
- fix(hosts): expand PreToolUse matcher to cover Read, Grep, Glob, Agent, Task (#86)
- fix(json_util,statusline): whitespace-tolerant JSON parser + UTF-8 statusline on Windows (#81)
- fix(hosts): preserve user settings.json when existing JSON fails to parse (#83)
- prevent OOM from unbounded file reads + data integrity fixes
- fix(ci): use native ubuntu-24.04-arm runner for aarch64 build

## [1.9.0] - 2026-04-22

## [1.7.7] - 2026-04-21

### Changed
- chore(release): bump version to 1.7.7 [squeez-release-bot]
- docs: update changelog and benchmarks for v1.7.6
- chore(release): bump version to 1.7.6 [squeez-release-bot]
- docs: update changelog and benchmarks for v1.7.5
- chore(release): bump version to 1.7.5 [squeez-release-bot]
- docs: update changelog and benchmarks for v1.7.4
- chore(release): bump version to 1.7.4 [squeez-release-bot]
- perf: O(log n) refactor — tail read, offset index, single-pass parser
- docs: update changelog and benchmarks for v1.7.3
- chore(release): bump version to 1.7.3 [squeez-release-bot]

### Fixed
- fix(json_util,statusline): whitespace-tolerant JSON parser + UTF-8 statusline on Windows (#81)
- fix(hosts): preserve user settings.json when existing JSON fails to parse (#83)
- prevent OOM from unbounded file reads + data integrity fixes
- fix(ci): use native ubuntu-24.04-arm runner for aarch64 build

## [1.7.6] - 2026-04-21

### Changed
- chore(release): bump version to 1.7.6 [squeez-release-bot]
- docs: update changelog and benchmarks for v1.7.5
- chore(release): bump version to 1.7.5 [squeez-release-bot]
- docs: update changelog and benchmarks for v1.7.4
- chore(release): bump version to 1.7.4 [squeez-release-bot]
- perf: O(log n) refactor — tail read, offset index, single-pass parser
- docs: update changelog and benchmarks for v1.7.3
- chore(release): bump version to 1.7.3 [squeez-release-bot]

### Fixed
- fix(hosts): preserve user settings.json when existing JSON fails to parse (#83)
- prevent OOM from unbounded file reads + data integrity fixes
- fix(ci): use native ubuntu-24.04-arm runner for aarch64 build

## [1.7.5] - 2026-04-19

### Changed
- chore(release): bump version to 1.7.5 [squeez-release-bot]
- docs: update changelog and benchmarks for v1.7.4
- chore(release): bump version to 1.7.4 [squeez-release-bot]
- perf: O(log n) refactor — tail read, offset index, single-pass parser
- docs: update changelog and benchmarks for v1.7.3
- chore(release): bump version to 1.7.3 [squeez-release-bot]

### Fixed
- prevent OOM from unbounded file reads + data integrity fixes
- fix(ci): use native ubuntu-24.04-arm runner for aarch64 build

## [1.7.4] - 2026-04-19

### Changed
- chore(release): bump version to 1.7.4 [squeez-release-bot]
- perf: O(log n) refactor — tail read, offset index, single-pass parser
- docs: update changelog and benchmarks for v1.7.3
- chore(release): bump version to 1.7.3 [squeez-release-bot]

### Fixed
- fix(ci): use native ubuntu-24.04-arm runner for aarch64 build

## [1.7.3] - 2026-04-19

### Changed
- chore(release): bump version to 1.7.3 [squeez-release-bot]

### Fixed
- fix(ci): use native ubuntu-24.04-arm runner for aarch64 build

## [1.7.0] - 2026-04-18
### Features
- Efficiency-proof benchmark suite — quantitative token-savings proof for US-001 / US-003 / US-004 (#74)

## [1.6.1] - 2026-04-18
### Bug Fixes
- OpenCode plugin aligned with PluginModule SDK shape (#72)

## [1.6.0] - 2026-04-18
### Features
- Token-compression power-up — sig-mode, memory-file warn, structured summaries, hypothesis benchmark (#70)

## [1.5.2] - 2026-04-18
### Bug Fixes
- CI: push release docs directly to main (loop-safe)

## [1.5.1] and earlier
See the [git tag history](https://github.com/claudioemmanuel/squeez/tags) for pre-1.5.2 details. release-please takes over changelog generation from 1.7.1 onwards.

[Unreleased]: https://github.com/claudioemmanuel/squeez/compare/v1.11.0...HEAD
[1.7.0]: https://github.com/claudioemmanuel/squeez/compare/v1.6.1...v1.7.0
[1.6.1]: https://github.com/claudioemmanuel/squeez/compare/v1.6.0...v1.6.1
[1.6.0]: https://github.com/claudioemmanuel/squeez/compare/v1.5.2...v1.6.0
[1.5.2]: https://github.com/claudioemmanuel/squeez/releases/tag/v1.5.2
[1.7.3]: https://github.com/claudioemmanuel/squeez/compare/v1.7.2...v1.7.3
[1.7.4]: https://github.com/claudioemmanuel/squeez/compare/v1.7.2...v1.7.4
[1.7.5]: https://github.com/claudioemmanuel/squeez/compare/v1.7.2...v1.7.5
[1.7.6]: https://github.com/claudioemmanuel/squeez/compare/v1.7.2...v1.7.6
[1.7.7]: https://github.com/claudioemmanuel/squeez/compare/v1.7.2...v1.7.7
[1.9.0]: https://github.com/claudioemmanuel/squeez/compare/v1.10.0...v1.9.0
[1.10.0]: https://github.com/claudioemmanuel/squeez/compare/v1.7.2...v1.10.0
[1.11.0]: https://github.com/claudioemmanuel/squeez/releases/tag/v1.11.0
