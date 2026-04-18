#!/usr/bin/env bash
# squeez Codex CLI SessionStart hook — finalizes previous session and
# refreshes ~/.codex/AGENTS.md, which Codex CLI auto-loads at session start.
#
# Registered in ~/.codex/hooks.json under hooks.SessionStart.
set -euo pipefail

SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

export SQUEEZ_DIR="$HOME/.codex/squeez"
"$SQUEEZ" init --host=codex 2>/dev/null || true
