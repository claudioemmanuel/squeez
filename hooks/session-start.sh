#!/usr/bin/env bash
# squeez SessionStart hook — runs squeez init, prints memory banner to session context
SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0
"$SQUEEZ" init

# Hook health check — warn if squeez hooks were removed from settings.json
# (e.g. by another tool like OMC that overwrites the file on setup).
_settings="$HOME/.claude/settings.json"
if [ -f "$_settings" ]; then
    _has_squeez=$(python3 -c "
import json, sys
try:
    d = json.load(open(sys.argv[1]))
    hooks = d.get('hooks', {}) if isinstance(d, dict) else {}
    for entries in hooks.values():
        for e in (entries if isinstance(entries, list) else []):
            for h in (e.get('hooks', []) if isinstance(e, dict) else []):
                if 'squeez' in str(h.get('command', '')):
                    print('ok'); sys.exit(0)
    print('missing')
except Exception:
    print('ok')
" "$_settings" 2>/dev/null || echo "ok")
    if [ "$_has_squeez" = "missing" ]; then
        printf '\n[squeez] WARNING: hooks not registered in %s\n' "$_settings"
        printf '[squeez] Another tool may have overwritten your settings. Run: squeez setup\n\n'
    fi
fi

# Rate-limited update check (at most once per day).
# Outputs a notification when a new squeez version is available.
_uc_ts="$HOME/.claude/squeez/.update-check-ts"
_now=$(date +%s 2>/dev/null || echo 0)
_last=0
[ -f "$_uc_ts" ] && _last=$(cat "$_uc_ts" 2>/dev/null || echo 0)
_diff=$(( _now - _last )) 2>/dev/null || _diff=99999
if [ "$_diff" -gt 86400 ]; then
    echo "$_now" > "$_uc_ts" 2>/dev/null || true
    _uc_out=$("$SQUEEZ" update --check 2>/dev/null || true)
    if echo "$_uc_out" | grep -q "→"; then
        printf "\n[squeez] Update available: %s\nRun: squeez update\n" "$_uc_out" >&2
    fi
fi
