#!/usr/bin/env bash
rm -rf "$HOME/.claude/squeez"
python3 - <<'EOF'
import json, os
path = os.path.expanduser("~/.claude/settings.json")
if not os.path.exists(path): exit()
with open(path) as f: s = json.load(f)
s["PreToolUse"] = [h for h in s.get("PreToolUse",[]) if "squeez" not in str(h)]
with open(path, "w") as f: json.dump(s, f, indent=2)
EOF
echo "✅ squeez uninstalled. Restart Claude Code."
