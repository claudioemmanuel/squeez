#!/usr/bin/env bash
# squeez Codex CLI PostToolUse hook — records results into SessionContext.
# Tracking only: Codex treats exit 0 with no stdout as success, so this must
# print nothing. The logic lives in `squeez hook record-result`.
#
# Registered in ~/.codex/hooks.json under hooks.PostToolUse.
SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

export SQUEEZ_DIR="$HOME/.codex/squeez"

"$SQUEEZ" hook record-result >/dev/null 2>&1 || true
