#!/usr/bin/env bash
set -euo pipefail
REPO_RAW="https://raw.githubusercontent.com/claudioemmanuel/squeez/main"
RELEASES="https://github.com/claudioemmanuel/squeez/releases/latest/download"
INSTALL_DIR="$HOME/.claude/squeez"

mkdir -p "$INSTALL_DIR/bin" "$INSTALL_DIR/hooks" "$INSTALL_DIR/sessions" "$INSTALL_DIR/memory"

echo "Downloading squeez binary..."
curl -fsSL "$RELEASES/squeez-macos-universal" -o "$INSTALL_DIR/bin/squeez"
chmod +x "$INSTALL_DIR/bin/squeez"

echo "Installing hooks..."
curl -fsSL "$REPO_RAW/hooks/pretooluse.sh" -o "$INSTALL_DIR/hooks/pretooluse.sh"
chmod +x "$INSTALL_DIR/hooks/pretooluse.sh"

echo "Registering hook in ~/.claude/settings.json..."
python3 - <<'EOF'
import json, os, sys
path = os.path.expanduser("~/.claude/settings.json")
settings = {}
try:
    if os.path.exists(path):
        with open(path) as f:
            settings = json.load(f)
except (json.JSONDecodeError, IOError) as e:
    print(f"Warning: could not read settings.json: {e}", file=sys.stderr)
if not isinstance(settings.get("PreToolUse"), list):
    settings["PreToolUse"] = []
hook = {"matcher": "Bash", "hooks": [{"type": "command", "command": "bash ~/.claude/squeez/hooks/pretooluse.sh"}]}
pre = settings["PreToolUse"]
if not any("squeez" in str(h) for h in pre):
    pre.append(hook)
tmp = path + ".tmp"
with open(tmp, "w") as f:
    json.dump(settings, f, indent=2)
os.replace(tmp, path)
EOF

echo "✅ $($INSTALL_DIR/bin/squeez --version) installed. Restart Claude Code to activate."
