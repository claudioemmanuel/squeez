#!/usr/bin/env bash
# squeez Codex CLI PostToolUse hook — records results into SessionContext.
#
# Registered in ~/.codex/hooks.json under hooks.PostToolUse.
set -euo pipefail

SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

export SQUEEZ_DIR="$HOME/.codex/squeez"

python3 -c "
import json, sys, subprocess, os

data = sys.stdin.read()
if not data.strip():
    sys.exit(0)
try:
    d = json.loads(data)
except json.JSONDecodeError:
    sys.exit(0)

tool = d.get('tool_name') or d.get('tool') or 'unknown'
try:
    subprocess.run(
        [os.environ.get('SQUEEZ') or '$SQUEEZ', 'track-result', tool],
        input=data,
        timeout=3,
        check=False,
    )
except Exception:
    pass
"
