#!/usr/bin/env bash
# squeez Codex CLI PostToolUse hook — records results into SessionContext.
#
# Registered in ~/.codex/hooks.json under hooks.PostToolUse.
set -euo pipefail

# Resolve a working Python interpreter.
#
# Hardcoding python3 is not safe on Windows: python.org installs ship
# python.exe with no python3.exe, so the name resolves to the Microsoft Store
# App Execution Alias under %LOCALAPPDATA%\Microsoft\WindowsApps. That stub is
# on PATH and passes `command -v`, but exits non-zero when run — which left
# every squeez hook silently dead (issue #209). Probe by EXECUTING each
# candidate, not by locating it.
SQUEEZ_PY=""
for _c in python3 python py; do
    if command -v "$_c" >/dev/null 2>&1 && "$_c" -c "" >/dev/null 2>&1; then
        SQUEEZ_PY="$_c"
        break
    fi
done
[ -z "$SQUEEZ_PY" ] && exit 0

SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

export SQUEEZ_DIR="$HOME/.codex/squeez"
export SQUEEZ_BIN="$SQUEEZ"

"$SQUEEZ_PY" -c "
import json, sys, subprocess, os

data = sys.stdin.read()
if not data.strip():
    sys.exit(0)
try:
    d = json.loads(data)
except json.JSONDecodeError:
    sys.exit(0)

tool = d.get('tool_name') or d.get('tool') or 'unknown'
# Tracking-only hook: record state, emit nothing. Codex treats exit 0 with no
# stdout as success, so suppress any subprocess output to keep the channel clean.
try:
    subprocess.run(
        [os.environ['SQUEEZ_BIN'], 'track-result', tool],
        input=data,
        timeout=3,
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
except Exception:
    pass
"
