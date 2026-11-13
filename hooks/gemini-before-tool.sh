#!/usr/bin/env bash
# squeez Gemini CLI BeforeTool hook — rewrites shell commands with
# `squeez wrap` before execution. The logic lives in
# `squeez hook gemini-before-tool` (src/commands/hook/shell.rs). Fails open.
#
# Registered in ~/.gemini/settings.json under hooks.BeforeTool.
SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

SQUEEZ_BIN="$SQUEEZ" "$SQUEEZ" hook gemini-before-tool 2>/dev/null || true
