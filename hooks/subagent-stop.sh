#!/usr/bin/env bash
# squeez SubagentStop hook — feeds sub-agent final output into SessionContext.
#
# SubagentStop fires when a sub-agent spawned via Agent/Task completes.
# Payload includes last_assistant_message (top-level), agent_id, and
# agent_transcript_path. We extract file paths and errors from the final
# message so the parent agent can dedup against what the sub-agent already saw.
set -euo pipefail

SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

input=$(cat)

# Wrap last_assistant_message into a tool_result-compatible JSON so that
# track-result's existing extract_content() logic picks it up correctly.
wrapped=$(printf '%s' "$input" | python3 -c "
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
fi

# Track sub-agent spawn cost (~200K tokens/spawn heuristic, size=0 for now)
"$SQUEEZ" track SubagentStop 0 2>/dev/null || true
