## Linked issue

Closes #<!-- REQUIRED: issue number. PRs without a linked issue are auto-rejected by CI (.github/workflows/pr-checks.yml). -->

## Type of change

<!-- Pick the one that matches the commit prefix. auto-release.yml reads the prefix to decide the version bump. -->

- [ ] `feat:` — new functionality (→ minor bump)
- [ ] `fix:` / `perf:` — bug fix or perf improvement (→ patch bump)
- [ ] `chore:` / `docs:` / `ci:` / `test:` / `refactor:` — no release
- [ ] Breaking change — `!:` or `BREAKING CHANGE:` in commit body (→ major bump)

## Area

<!-- Which subsystem does this touch? Multi-select. -->

- [ ] Handler (`src/commands/*`)
- [ ] Compression pipeline (`src/strategies/` — dedup / grouping / truncation / smart_filter)
- [ ] Context engine (`src/context/` — cache / redundancy / summarize / intensity)
- [ ] MCP server (`src/commands/mcp_server.rs`)
- [ ] CI / release / tooling
- [ ] Docs

## Summary

<!-- What does this PR do? 2-3 bullets. -->

## Test plan

- [ ] `cargo test` passes
- [ ] `bash bench/run.sh` — all fixtures ≥30% reduction, ≤100ms latency
- [ ] `bash bench/run_context.sh` — all context scenarios pass (if touching context engine)

## Checklist

- [ ] Issue is linked above
- [ ] CI is green
- [ ] No `Co-Authored-By:` trailers in commit messages
- [ ] Zero-dependency constraint respected (only `libc` in `Cargo.toml`)
