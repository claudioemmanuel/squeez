# Contributing to squeez

## License + Contributor sign-off

squeez is licensed under **Apache License 2.0** (see [LICENSE](LICENSE)).

Every contribution must be signed off under the
[Developer Certificate of Origin (DCO) 1.1](https://developercertificate.org/).
Signing off is a trailer in every commit:

```
Signed-off-by: Your Real Name <your.email@example.com>
```

The easy way — add `-s` to every commit:

```bash
git commit -s -m "your commit message"
```

Or configure git once per clone to always sign off:

```bash
git config --local format.signoff true
```

The signoff is a lightweight affirmation that you wrote (or have the right to
contribute) the patch under the project's license. It's **not a CLA** — there's
no separate document to sign, and you keep the copyright on what you contribute.

A DCO workflow check blocks any PR whose commits lack a valid `Signed-off-by:`
line until it's added.

## Releases

### How the release pipeline works

1. **Every merge to `main`** triggers `release-please.yml`, which opens or updates a "Release PR" at the top of the PR list. The PR title is always `chore(main): release X.Y.Z`.
2. **Merging the Release PR** causes release-please to tag the repo (`vX.Y.Z`) and push the tag using `RELEASE_PAT`. That tag push triggers `release.yml`, which builds platform binaries (macOS universal, Linux x86_64/aarch64 musl, Windows MSVC), runs smoke tests, publishes the GitHub Release, then publishes to npm and crates.io.
3. **`CHANGELOG.md` is auto-managed** by release-please — do not edit it by hand except through the Release PR. Sections are generated from conventional commit messages.

### Conventional commit bump rules

| Prefix | Version bump |
|--------|-------------|
| `feat:` | minor |
| `fix:` / `perf:` | patch |
| `feat!:` / `fix!:` / `BREAKING CHANGE:` in body | major |
| `chore:` / `docs:` / `test:` / `ci:` / `refactor:` | no bump (no release) |

### Prerelease / @next dist-tag

When a tag contains a prerelease identifier (e.g. `v2.0.0-alpha.1`, `v2.0.0-beta.2`, `v2.0.0-rc.1`), `release.yml` publishes to npm with `--tag next` instead of `--tag latest`. To opt into prereleases:

```bash
npm install squeez@next
```

To configure release-please to generate prerelease tags, set `"prerelease": true` and `"prerelease-type": "alpha"` (or `beta`/`rc`) in `release-please-config.json`. Currently disabled — `"prerelease": false`.

### GPG signed tags (optional)

By default tags are unsigned. To enable signed tags:

1. Generate a key: `gpg --full-generate-key`
2. Export the public key: `gpg --armor --export KEYID` → add to your GitHub profile under Settings → SSH and GPG keys
3. Add the private key to repo secrets as `RELEASE_GPG_PRIVATE_KEY`
4. Add the passphrase to repo secrets as `RELEASE_GPG_PASSPHRASE`
5. Configure git locally:
   ```bash
   git config --global user.signingkey KEYID
   git config --global commit.gpgsign true
   git config --global tag.gpgSign true
   ```
6. Uncomment the GPG import block inside `.github/workflows/release-please.yml` (see the comments there).

## Adding a new command handler

1. Create `src/commands/newcmd.rs` implementing `Handler` trait
2. Write tests in `tests/test_newcmd.rs`
3. Add a real fixture: `bash bench/capture.sh "newcmd args" > bench/fixtures/newcmd.txt`
4. Register in `src/commands/mod.rs` and `src/filter.rs`
5. Run: `cargo test && bash bench/run.sh`
6. Open a PR

## Adding a fixture

```bash
bash bench/capture.sh "your command" > bench/fixtures/your_command.txt
```

## Testing the MCP server

The MCP server (`squeez mcp`) is tested via `tests/test_mcp_server.rs` — these tests call `handle_request()` directly without a running process. To exercise the wire protocol end-to-end:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize"}' | ./target/release/squeez mcp
```

When adding new MCP tools, add them to:
- `src/commands/mcp_server.rs` — `handle_tools_list()` and `handle_tools_call()`
- `src/commands/protocol.rs` — mention in `SQUEEZ_PROTOCOL` (the auto-teach payload)
- `tests/test_mcp_server.rs` — at minimum a `tools/list` check

## Context engine changes

Changes to `src/context/` should be tested against the 16-call sliding window edge cases. Key invariants:
- Exact-hash dedup: `RECENT_WINDOW = 16`, `MIN_LINES = 2`
- Fuzzy dedup: `MIN_LINES_FUZZY = 6`, Jaccard ≥ `SIMILARITY_THRESHOLD = 0.85`, length ratio ≥ `LENGTH_RATIO_GUARD = 0.80`
- Adaptive intensity: Full below 80% of `budget(cfg)`, Ultra above. Budget = `compact_threshold_tokens × 5/4`
- Benign summarize: `BENIGN_MULTIPLIER = 2`, threshold doubled when no error markers found
