#!/usr/bin/env bash
# squeez Copilot CLI PostToolUse hook — tracks token usage per tool call
SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
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

# Tolerant extraction: tries tool_response (Claude Code convention) then
# tool_result; handles string, {content: str|blocks}, and nested file.content.
size=$(printf '%s' "$input" | python3 -c "
import sys, json
def text_len(c):
    if c is None:
        return 0
    if isinstance(c, str):
        return len(c)
    if isinstance(c, list):
        return sum(len(b.get('text', '')) for b in c if isinstance(b, dict))
    if isinstance(c, dict):
        if 'content' in c:
            return text_len(c['content'])
        if isinstance(c.get('file'), dict):
            return text_len(c['file'].get('content'))
        if 'text' in c:
            return text_len(c.get('text'))
        return 0
    return len(str(c))
try:
    d = json.load(sys.stdin)
    r = d.get('tool_response', d.get('tool_result', {}))
    print(text_len(r))
except Exception:
    print(0)
" 2>/dev/null || echo 0)

"$SQUEEZ" track "$tool" "$size" 2>/dev/null || true

# Also feed the raw JSON to track-result so non-Bash tool outputs
# (Read, Grep, LS, Glob) update SessionContext for cross-call dedup.
printf '%s' "$input" | "$SQUEEZ" track-result "$tool" 2>/dev/null || true
