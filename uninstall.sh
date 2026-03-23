#!/usr/bin/env bash
set -euo pipefail

SQUEEZ_DIR="$HOME/.claude/squeez"

if [ ! -d "$SQUEEZ_DIR" ]; then
    echo "ℹ️  squeez not installed (no $SQUEEZ_DIR found)"
    exit 0
fi

if [ -L "$SQUEEZ_DIR" ]; then
    echo "❌ Error: $SQUEEZ_DIR is a symlink. Refusing to remove."
    exit 1
fi

rm -rf "$SQUEEZ_DIR"

python3 - <<'EOF'
import json, os, sys
path = os.path.expanduser("~/.claude/settings.json")
if not os.path.exists(path):
    exit(0)
try:
    with open(path) as f:
        s = json.load(f)
except (json.JSONDecodeError, IOError) as e:
    print(f"⚠️  Warning: could not read settings.json: {e}", file=sys.stderr)
    exit(0)
s["PreToolUse"] = [h for h in s.get("PreToolUse", []) if "squeez" not in str(h)]
tmp = path + ".tmp"
with open(tmp, "w") as f:
    json.dump(s, f, indent=2)
os.replace(tmp, path)
EOF

echo "✅ squeez uninstalled. Restart Claude Code."
