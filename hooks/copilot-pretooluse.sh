#!/usr/bin/env bash
# squeez Copilot CLI PreToolUse hook — Bash → `squeez wrap` behind the #150
# safety gate, and size-budget limits for Read/Grep/Glob (parity with the
# Claude Code hook). The logic lives in `squeez hook copilot-pre-tool-use`
# (src/commands/hook/shell.rs); this only finds the binary. Fails open.
#
# Registered in ~/.copilot/settings.json under hooks.PreToolUse.
SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

export SQUEEZ_DIR="$HOME/.copilot/squeez"

SQUEEZ_BIN="$SQUEEZ" "$SQUEEZ" hook copilot-pre-tool-use 2>/dev/null || true
