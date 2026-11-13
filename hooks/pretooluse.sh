#!/usr/bin/env bash
# squeez PreToolUse hook. The logic lives in `squeez hook pre-tool-use`
# (src/commands/hook/mod.rs): Bash → `squeez wrap` behind the #150 safety gate,
# Read/Grep/Glob limits, the agent-spawn ceiling, Agent/Task prompt compression.
#
# It used to be Python embedded here: an interpreter probe, a Python run and a
# `should-wrap` process on every tool call, ~40ms before the tool started.
#
# Fails open: no binary, or any error, leaves the tool call untouched. Only the
# JSON `deny` of the spawn ceiling ever blocks one — never this exit status.
SQUEEZ="${SQUEEZ_BIN:-$HOME/.claude/squeez/bin/squeez}"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

SQUEEZ_BIN="$SQUEEZ" "$SQUEEZ" hook pre-tool-use 2>/dev/null || true
