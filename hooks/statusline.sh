#!/usr/bin/env bash
# squeez statusline — emits a compact status line for Claude Code statusLine hook.
# Reads session data from ${SQUEEZ_DIR:-~/.claude/squeez}; the logic lives in
# `squeez hook statusline` (src/commands/hook/statusline.rs).
SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

"$SQUEEZ" hook statusline 2>/dev/null || true
