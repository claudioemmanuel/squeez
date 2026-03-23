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
