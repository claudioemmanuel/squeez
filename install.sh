#!/usr/bin/env bash
set -euo pipefail
REPO_RAW="https://raw.githubusercontent.com/claudioemmanuel/squeez/main"
RELEASES="https://github.com/claudioemmanuel/squeez/releases/latest/download"
INSTALL_DIR="$HOME/.claude/squeez"

# Detect OS and architecture
OS=$(uname -s)
ARCH=$(uname -m)

case "$OS" in
  Darwin)  BINARY="squeez-macos-universal" ;;
  Linux)
    case "$ARCH" in
      x86_64)          BINARY="squeez-linux-x86_64" ;;
      aarch64|arm64)   BINARY="squeez-linux-aarch64" ;;
      *) echo "ERROR: unsupported arch $ARCH" >&2; exit 1 ;;
    esac
    ;;
  Windows*|MINGW*|CYGWIN*)
    echo "ERROR: Windows não é suportado. Use macOS ou Linux." >&2
    exit 1
    ;;
  *) echo "ERROR: unsupported OS $OS" >&2; exit 1 ;;
esac

mkdir -p "$INSTALL_DIR/bin" "$INSTALL_DIR/hooks" "$INSTALL_DIR/sessions" "$INSTALL_DIR/memory"
chmod 700 "$INSTALL_DIR" "$INSTALL_DIR/sessions" "$INSTALL_DIR/memory"

echo "Downloading squeez binary for $OS/$ARCH..."
curl -fsSL "$RELEASES/$BINARY" -o "$INSTALL_DIR/bin/squeez"

echo "Verifying checksum..."
curl -fsSL "$RELEASES/checksums.sha256" -o /tmp/squeez-checksums.sha256
expected=$(grep "$BINARY" /tmp/squeez-checksums.sha256 2>/dev/null | awk '{print $1}')
rm -f /tmp/squeez-checksums.sha256
if [ -z "$expected" ]; then
    echo "ERROR: could not find checksum for $BINARY in release" >&2
    rm -f "$INSTALL_DIR/bin/squeez"
    exit 1
fi

# Use sha256sum if available (Linux), otherwise fall back to shasum (macOS)
if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$INSTALL_DIR/bin/squeez" | awk '{print $1}')
else
    actual=$(shasum -a 256 "$INSTALL_DIR/bin/squeez" | awk '{print $1}')
fi

if [ "$expected" != "$actual" ]; then
    echo "ERROR: checksum mismatch — binary may be corrupted or tampered" >&2
    rm -f "$INSTALL_DIR/bin/squeez"
    exit 1
fi
echo "Checksum verified."

chmod +x "$INSTALL_DIR/bin/squeez"

echo "Installing hooks..."
curl -fsSL "$REPO_RAW/hooks/pretooluse.sh"     -o "$INSTALL_DIR/hooks/pretooluse.sh"
curl -fsSL "$REPO_RAW/hooks/session-start.sh"  -o "$INSTALL_DIR/hooks/session-start.sh"
curl -fsSL "$REPO_RAW/hooks/posttooluse.sh"    -o "$INSTALL_DIR/hooks/posttooluse.sh"
chmod +x "$INSTALL_DIR/hooks/pretooluse.sh" "$INSTALL_DIR/hooks/session-start.sh" "$INSTALL_DIR/hooks/posttooluse.sh"

echo "Registering hooks in ~/.claude/settings.json..."
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

# PreToolUse — Bash compression
if not isinstance(settings.get("PreToolUse"), list):
    settings["PreToolUse"] = []
pre_hook = {"matcher": "Bash", "hooks": [{"type": "command", "command": "bash ~/.claude/squeez/hooks/pretooluse.sh"}]}
if not any("squeez" in str(h) for h in settings["PreToolUse"]):
    settings["PreToolUse"].append(pre_hook)

# SessionStart — init + memory banner
if not isinstance(settings.get("SessionStart"), list):
    settings["SessionStart"] = []
start_hook = {"hooks": [{"type": "command", "command": "bash ~/.claude/squeez/hooks/session-start.sh"}]}
if not any("squeez" in str(h) for h in settings["SessionStart"]):
    settings["SessionStart"].append(start_hook)

# PostToolUse — token tracking (no matcher = fires after all tools, not just Bash)
if not isinstance(settings.get("PostToolUse"), list):
    settings["PostToolUse"] = []
post_hook = {"hooks": [{"type": "command", "command": "bash ~/.claude/squeez/hooks/posttooluse.sh"}]}
if not any("squeez" in str(h) for h in settings["PostToolUse"]):
    settings["PostToolUse"].append(post_hook)

tmp = path + ".tmp"
with open(tmp, "w") as f:
    json.dump(settings, f, indent=2)
os.replace(tmp, path)
EOF

version=$("$INSTALL_DIR/bin/squeez" --version 2>/dev/null || echo "squeez")
echo "✅ $version installed. Restart Claude Code to activate."
