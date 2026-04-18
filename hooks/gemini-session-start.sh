#!/usr/bin/env bash
# squeez Gemini CLI SessionStart hook — emits session memory to stdout
# which Gemini CLI injects into the initial system prompt. Also writes to
# ~/.gemini/GEMINI.md so the summary persists across sessions.
#
# Registered in ~/.gemini/settings.json under hooks.SessionStart.
set -euo pipefail

SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

export SQUEEZ_DIR="$HOME/.gemini/squeez"
"$SQUEEZ" init --host=gemini 2>/dev/null || true
