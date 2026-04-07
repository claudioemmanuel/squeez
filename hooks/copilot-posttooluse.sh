#!/usr/bin/env bash
# squeez Copilot CLI PostToolUse hook — tracks token usage per tool call
SQUEEZ="$HOME/.claude/squeez/bin/squeez"
[ ! -x "$SQUEEZ" ] && exit 0

export SQUEEZ_DIR="$HOME/.copilot/squeez"

input=$(cat)

tool=$(printf '%s' "$input" | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
    print(d.get('tool_name', 'unknown'))
except Exception:
    print('unknown')
" 2>/dev/null || echo "unknown")

size=$(printf '%s' "$input" | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
    content = d.get('tool_result', {})
    if isinstance(content, dict):
        content = str(content.get('content', ''))
    elif content is None:
        content = ''
    else:
        content = str(content)
    print(len(content))
except Exception:
    print(0)
" 2>/dev/null || echo 0)

"$SQUEEZ" track "$tool" "$size" 2>/dev/null || true

# Also feed the raw JSON to track-result so non-Bash tool outputs
# (Read, Grep, LS, Glob) update SessionContext for cross-call dedup.
printf '%s' "$input" | "$SQUEEZ" track-result "$tool" 2>/dev/null || true
