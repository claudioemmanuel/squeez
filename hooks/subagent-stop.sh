#!/usr/bin/env bash
# squeez SubagentStop hook — feeds sub-agent final output into SessionContext.
#
# SubagentStop fires when a sub-agent spawned via Agent/Task completes.
# Payload includes last_assistant_message (top-level), agent_id, and
# agent_transcript_path. We extract file paths and errors from the final
# message so the parent agent can dedup against what the sub-agent already saw.
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

SQUEEZ="${SQUEEZ_BIN:-$HOME/.claude/squeez/bin/squeez}"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

input=$(cat)

# Measure what this sub-agent actually cost, from its own transcript. This is
# where estimation stops: at dispatch the turn count is unknowable, but by now
# the turns have happened and are on disk.
printf '%s' "$input" | "$SQUEEZ" track-agent-cost 2>/dev/null || true

# Release one slot from the in-flight spawn ledger written by pretooluse.sh.
# Drops the OLDEST stamp rather than matching an id: the PreToolUse hook has no
# agent id to record at dispatch time, and over-releasing is the safe direction
# — a lost slot re-opens capacity, while a leaked one would throttle the session
# until the TTL expires. Pruning by TTL there makes both errors self-correcting.
"$SQUEEZ_PY" -c "
import os, sys
try:
    p = os.path.join(os.path.expanduser('~'), '.claude', 'squeez', 'sessions', 'inflight_agents')
    with open(p) as fh:
        stamps = [x for x in fh.read().split() if x.strip().isdigit()]
    if stamps:
        stamps.pop(0)
        tmp = p + '.tmp'
        with open(tmp, 'w') as fh:
            fh.write('\n'.join(stamps))
        os.replace(tmp, p)
except Exception:
    pass
" 2>/dev/null || true

# Wrap last_assistant_message into a tool_result-compatible JSON so that
# track-result's existing extract_content() logic picks it up correctly.
wrapped=$(printf '%s' "$input" | "$SQUEEZ_PY" -c "
import json, sys
try:
    d = json.load(sys.stdin)
    msg = d.get('last_assistant_message', '')
    if not isinstance(msg, str):
        msg = str(msg)
    # Emit a synthetic tool_result payload for track-result
    print(json.dumps({
        'tool_name': 'SubagentStop',
        'tool_result': {'content': msg},
        'agent_id': d.get('agent_id', ''),
    }))
except Exception:
    sys.exit(0)
" 2>/dev/null || true)

if [ -n "$wrapped" ]; then
    printf '%s' "$wrapped" | "$SQUEEZ" track-result SubagentStop 2>/dev/null || true
    # Emit an `additionalContext` advisory when the sub-agent returned an
    # oversized result. SubagentStop cannot rewrite the returned message (no
    # updatedToolOutput for Stop-family events), so this only nudges future
    # sub-agents to return a summary + file path. stdout is read by the host.
    printf '%s' "$wrapped" | "$SQUEEZ" compress-output SubagentStop 2>/dev/null || true
fi

# Track sub-agent spawn cost (~200K tokens/spawn heuristic, size=0 for now)
"$SQUEEZ" track SubagentStop 0 2>/dev/null || true
