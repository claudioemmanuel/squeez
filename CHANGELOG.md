# Changelog

All notable changes to squeez are documented here.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [1.7.1] - 2026-04-19

### Added
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

## [1.7.0] - 2026-04-19

### Added
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

## [1.6.1] - 2026-04-19

### Added
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

## [1.6.0] - 2026-04-19

### Added
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

## [1.5.2] - 2026-04-18

### Added
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

## [0.4.1] - 2026-04-16

### Added
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

## [0.4.0] - 2026-04-16

### Added
- feat(opus47): dual-model cost table in benchmark — Sonnet 4.6 ($3/MTok) + Opus 4.7 ($5/MTok)
- feat(opus47): protocol now educates LLM on xhigh thinking-block overhead (~2× budget burn)

### Changed
- feat(opus47): compact_threshold_tokens 120k → 90k (earlier compaction trigger for new tokenizer)
- feat(opus47): agent_spawn_cost 270k → 350k (xhigh subagents burn more tokens)
- feat(opus47): burn_rate_warn_calls 20 → 30 (wider pre-exhaustion warning window)
- feat(opus47): state_warn_calls 5 → 10 (more runway to save session state)
- feat(opus47): ultra_trigger_pct 0.72 → 0.65 (go Ultra compression earlier)
- feat(opus47): MCP context_pressure thresholds — use_state_first ≥90%→≥75%, compact_soon ≥70%→≥55%
- fix: DEFAULT_AGENT_SPAWN_COST constant synced to config default (200k → 270k)

## [0.3.2] - 2026-04-16

### Added
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

### Fixed
- `squeez update` on Windows now detects cargo-managed installs and delegates to `cargo install squeez` automatically, avoiding exe-lock issues
- `install_target_path()` uses `current_exe()` instead of hardcoded `~/.claude/squeez/bin`, so the correct binary is always updated regardless of install method
- Windows self-update fallback spawns a detached `cmd.exe` to move the staged binary after process exit, instead of requiring manual `move /Y`
- `install_atomic` now returns `Ok(bool)` distinguishing immediate vs deferred installs; success message no longer appears when install was only staged

### Added
- `session-start.sh`: rate-limited daily update check — prints `[squeez] Update available` to stderr (visible in terminal) when a new version is detected
- `squeez setup`: auto-detects user language from `~/.claude/CLAUDE.md` and system locale (`LANG`/`LC_ALL`) and writes correct `lang =` into `config.ini` on first install

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

[Unreleased]: https://github.com/claudioemmanuel/squeez/compare/v1.7.1...HEAD
[0.3.0]: https://github.com/claudioemmanuel/squeez/compare/v0.2.3...v0.3.0
[0.2.3]: https://github.com/claudioemmanuel/squeez/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/claudioemmanuel/squeez/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/claudioemmanuel/squeez/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/claudioemmanuel/squeez/releases/tag/v0.2.0
[0.3.2]: https://github.com/claudioemmanuel/squeez/releases/tag/v0.3.2
[0.4.1]: https://github.com/claudioemmanuel/squeez/releases/tag/v0.4.1
[1.5.2]: https://github.com/claudioemmanuel/squeez/releases/tag/v1.5.2
[1.6.0]: https://github.com/claudioemmanuel/squeez/releases/tag/v1.6.0
[1.6.1]: https://github.com/claudioemmanuel/squeez/releases/tag/v1.6.1
[1.7.0]: https://github.com/claudioemmanuel/squeez/releases/tag/v1.7.0
[1.7.1]: https://github.com/claudioemmanuel/squeez/releases/tag/v1.7.1
