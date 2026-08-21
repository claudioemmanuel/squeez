#!/usr/bin/env bash
# squeez PostToolUse hook — tracks token usage and rewrites tool output when
# content is redundant or oversized (Claude Code v2.1.119+ updatedToolOutput).
# Bash compression is handled at PreToolUse via `squeez wrap`; this hook covers
# Read / Grep / Glob whose output bypasses the wrap mechanism.

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

tool=$(printf '%s' "$input" | "$SQUEEZ_PY" -c "
import sys, json
try:
    d = json.load(sys.stdin)
    print(d.get('tool_name', 'unknown'))
except Exception:
    print('unknown')
" 2>/dev/null || echo "unknown")

# Claude Code sends the output under top-level `tool_response`; `tool_result`
# is kept as a fallback for other hosts / older payloads. Shapes handled:
# plain string, {content: str}, {content: [{type:text,text:…}]}, and the Read
# shape {type:text, file:{content: …}} — mirrors track_result.rs extract_content.
size=$(printf '%s' "$input" | "$SQUEEZ_PY" -c "
import sys, json
def text_len(c):
    if c is None:
        return 0
    if isinstance(c, str):
        return len(c)
    if isinstance(c, list):
        # Blocks are usually {type:text,text:…} but a list of plain strings or
        # of nested shapes is just as valid — recurse instead of assuming.
        return sum(text_len(b) for b in c)
    if isinstance(c, dict):
        if 'content' in c:
            return text_len(c['content'])
        if isinstance(c.get('file'), dict):
            return text_len(c['file'].get('content'))
        if 'text' in c:
            return text_len(c.get('text'))
        # Binary blocks carry no text. Their cost is real but it is not
        # measured here — image dedup runs off `image_fp` in compress-output —
        # and summing a base64 payload as if it were prose would inflate every
        # screenshot into a fake 100K result.
        if c.get('type') in ('image', 'video', 'audio', 'document') or 'source' in c:
            return 0
        # UNKNOWN SHAPE. Returning 0 here is what made squeez blind to the
        # tools that cost the most: WebFetch/WebSearch answer with neither
        # `content` nor `text` at the top level, so every one of 797 calls in
        # the 2026-08-21 burn was recorded as 0 tokens, and every guard built
        # on those counters (intensity, budget, burn rate) was fed ~0. Measure
        # the payload we cannot name rather than pretending it is empty.
        return sum(text_len(v) for v in c.values())
    return len(str(c))
try:
    d = json.load(sys.stdin)
    r = d.get('tool_response', d.get('tool_result', {}))
    print(text_len(r))
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
    Read|Grep|Glob|Monitor|Agent|Task|Edit|Write|Skill|WebFetch|WebSearch|ToolSearch|TaskOutput|mcp__*)
        rewrite=$(printf '%s' "$input" | "$SQUEEZ" compress-output "$tool" 2>/dev/null || true)
        if [ -n "$rewrite" ]; then
            printf '%s\n' "$rewrite"
        fi
        ;;
esac

# Feed to track-result so SessionContext picks up files, errors, real context
# size (transcript_path), quota-error escalation, and sub-agent size guard.
printf '%s' "$input" | "$SQUEEZ" track-result "$tool" 2>/dev/null || true
