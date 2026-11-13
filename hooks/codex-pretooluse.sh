#!/usr/bin/env bash
# squeez Codex CLI PreToolUse hook — rewrites bash/shell commands with
# `squeez wrap` before execution. The logic lives in
# `squeez hook codex-pre-tool-use` (src/commands/hook/shell.rs). Fails open.
#
# Hook surface as of Codex 0.123.0: apply_patch now emits PreToolUse/PostToolUse
# (openai/codex#18391), but updatedInput is explicitly rejected by the runtime
# and read_file/grep still have no hook surface (openai/codex#18491).
# Soft budget for read_file/grep is communicated via ~/.codex/AGENTS.md,
# written by `squeez init --host=codex`.
#
# Registered in ~/.codex/hooks.json under hooks.PreToolUse.
SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

SQUEEZ_BIN="$SQUEEZ" "$SQUEEZ" hook codex-pre-tool-use 2>/dev/null || true
