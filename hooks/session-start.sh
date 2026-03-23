#!/usr/bin/env bash
# squeez SessionStart hook — runs squeez init, prints memory banner to session context
SQUEEZ="$HOME/.claude/squeez/bin/squeez"
[ ! -x "$SQUEEZ" ] && exit 0
"$SQUEEZ" init
