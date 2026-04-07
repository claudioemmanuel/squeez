# Contributing to squeez

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
