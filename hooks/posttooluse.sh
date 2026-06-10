#!/usr/bin/env bash
# squeez PostToolUse hook — tracks token usage and rewrites tool output when
# content is redundant or oversized (Claude Code v2.1.119+ updatedToolOutput).
# Bash compression is handled at PreToolUse via `squeez wrap`; this hook covers
# Read / Grep / Glob whose output bypasses the wrap mechanism.
SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

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

# For Read/Grep/Glob/Monitor/Edit/Write and MCP tools: attempt output rewrite
# via updatedToolOutput. compress-output prints hookSpecificOutput JSON if
# content is redundant (incl. byte-identical image payloads), oversized, or
# contains strippable boilerplate; prints nothing if the original should be
# kept as-is. MCP results are dedup-only (never summarized).
#
# ORDER MATTERS: compress-output must run BEFORE track-result. The A2
# stale-state guard inside compress-output inspects the file's access flag
# from the PREVIOUS call (Write = edited since last seen); track-result for
# the current Read would overwrite that flag before the guard could see it.
case "$tool" in
    Read|Grep|Glob|Monitor|Agent|Task|Edit|Write|Skill|mcp__*)
        rewrite=$(printf '%s' "$input" | "$SQUEEZ" compress-output "$tool" 2>/dev/null || true)
        if [ -n "$rewrite" ]; then
            printf '%s\n' "$rewrite"
        fi
        ;;
esac

# Feed to track-result so SessionContext picks up files, errors, real context
# size (transcript_path), quota-error escalation, and sub-agent size guard.
printf '%s' "$input" | "$SQUEEZ" track-result "$tool" 2>/dev/null || true
