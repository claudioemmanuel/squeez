#!/usr/bin/env bash
set -euo pipefail
REPO="$(cd "$(dirname "$0")" && pwd)"

# Install rustup if needed
if ! command -v cargo &>/dev/null; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi

cd "$REPO"
cargo build --release
BINARY="$REPO/target/release/squeez"

INSTALL_DIR="$HOME/.claude/squeez"
mkdir -p "$INSTALL_DIR/bin" "$INSTALL_DIR/hooks" "$INSTALL_DIR/sessions" "$INSTALL_DIR/memory"
cp "$BINARY" "$INSTALL_DIR/bin/squeez" && chmod +x "$INSTALL_DIR/bin/squeez"
cp "$REPO/hooks/"*.sh "$INSTALL_DIR/hooks/" && chmod +x "$INSTALL_DIR/hooks/"*.sh

# Commit binary to repo
mkdir -p "$REPO/bin"
cp "$BINARY" "$REPO/bin/squeez"

# Register PreToolUse hook
python3 - <<'EOF'
import json, os
path = os.path.expanduser("~/.claude/settings.json")
settings = {}
if os.path.exists(path):
    with open(path) as f: settings = json.load(f)
hook = {"matcher": "Bash", "hooks": [{"type": "command", "command": "bash ~/.claude/squeez/hooks/pretooluse.sh"}]}
pre = settings.setdefault("PreToolUse", [])
if not any("squeez" in str(h) for h in pre): pre.append(hook)
with open(path, "w") as f: json.dump(settings, f, indent=2)
print("✅ hook registered in ~/.claude/settings.json")
EOF

echo "✅ squeez $($INSTALL_DIR/bin/squeez --version) installed ($(du -sh $INSTALL_DIR/bin/squeez | cut -f1))"
echo "   Restart Claude Code to activate."
