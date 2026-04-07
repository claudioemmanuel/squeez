#!/usr/bin/env bash
set -euo pipefail
REPO_RAW="https://raw.githubusercontent.com/claudioemmanuel/squeez/main"
RELEASES="https://github.com/claudioemmanuel/squeez/releases/latest/download"
INSTALL_DIR="$HOME/.claude/squeez"

# Parse flags
SETUP_ONLY=0
for arg in "$@"; do
  case "$arg" in
    --setup-only) SETUP_ONLY=1 ;;
  esac
done

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
  Windows*|MINGW*|MSYS*|CYGWIN*)
    BINARY="squeez-windows-x86_64.exe"
    ;;
  *) echo "ERROR: unsupported OS $OS" >&2; exit 1 ;;
esac

# Local binary name: squeez.exe on Windows, squeez elsewhere
case "$OS" in
  Windows*|MINGW*|MSYS*|CYGWIN*) BIN_NAME="squeez.exe" ;;
  *) BIN_NAME="squeez" ;;
esac

mkdir -p "$INSTALL_DIR/bin" "$INSTALL_DIR/hooks" "$INSTALL_DIR/sessions" "$INSTALL_DIR/memory"
chmod 700 "$INSTALL_DIR" "$INSTALL_DIR/sessions" "$INSTALL_DIR/memory" 2>/dev/null || true

if [ "$SETUP_ONLY" -eq 1 ]; then
  # --setup-only: skip download, use existing squeez binary from PATH
  echo "Setup-only mode: skipping binary download..."
  EXISTING=$(command -v squeez 2>/dev/null || true)
  if [ -z "$EXISTING" ]; then
    echo "ERROR: squeez not found in PATH." >&2
    echo "Install first with: cargo install squeez" >&2
    exit 1
  fi
  echo "Found squeez at: $EXISTING"
  cp "$EXISTING" "$INSTALL_DIR/bin/$BIN_NAME"
  chmod +x "$INSTALL_DIR/bin/$BIN_NAME" 2>/dev/null || true
  echo "Binary copied to $INSTALL_DIR/bin/$BIN_NAME"
else
  echo "Downloading squeez binary for $OS/$ARCH..."
  curl -fsSL "$RELEASES/$BINARY" -o "$INSTALL_DIR/bin/$BIN_NAME"

  echo "Verifying checksum..."
  curl -fsSL "$RELEASES/checksums.sha256" -o /tmp/squeez-checksums.sha256
  expected=$(grep "$BINARY" /tmp/squeez-checksums.sha256 2>/dev/null | awk '{print $1}')
  rm -f /tmp/squeez-checksums.sha256
  if [ -z "$expected" ]; then
      echo "ERROR: could not find checksum for $BINARY in release" >&2
      rm -f "$INSTALL_DIR/bin/$BIN_NAME"
      exit 1
  fi

  # Use sha256sum if available (Linux/Windows Git Bash), otherwise fall back to shasum (macOS)
  if command -v sha256sum >/dev/null 2>&1; then
      actual=$(sha256sum "$INSTALL_DIR/bin/$BIN_NAME" | awk '{print $1}')
  else
      actual=$(shasum -a 256 "$INSTALL_DIR/bin/$BIN_NAME" | awk '{print $1}')
  fi

  if [ "$expected" != "$actual" ]; then
      echo "ERROR: checksum mismatch — binary may be corrupted or tampered" >&2
      rm -f "$INSTALL_DIR/bin/$BIN_NAME"
      exit 1
  fi
  echo "Checksum verified."

  chmod +x "$INSTALL_DIR/bin/$BIN_NAME" 2>/dev/null || true
fi

echo "Installing hooks..."
curl -fsSL "$REPO_RAW/hooks/pretooluse.sh"     -o "$INSTALL_DIR/hooks/pretooluse.sh"
curl -fsSL "$REPO_RAW/hooks/session-start.sh"  -o "$INSTALL_DIR/hooks/session-start.sh"
curl -fsSL "$REPO_RAW/hooks/posttooluse.sh"    -o "$INSTALL_DIR/hooks/posttooluse.sh"
chmod +x "$INSTALL_DIR/hooks/pretooluse.sh" "$INSTALL_DIR/hooks/session-start.sh" "$INSTALL_DIR/hooks/posttooluse.sh"

echo "Installing status line script..."
curl -fsSL "$REPO_RAW/scripts/statusline.sh" -o "$INSTALL_DIR/bin/statusline.sh"
chmod +x "$INSTALL_DIR/bin/statusline.sh"

echo "Installing OpenCode plugin..."
OPENCODE_PLUGIN_DIR="$HOME/.config/opencode/plugins"
mkdir -p "$OPENCODE_PLUGIN_DIR"
curl -fsSL "$REPO_RAW/opencode-plugin/squeez.js" -o "$OPENCODE_PLUGIN_DIR/squeez.js" 2>/dev/null || {
    cat > "$OPENCODE_PLUGIN_DIR/squeez.js" <<'PLUGIN_EOF'
const SQUEEZ_BIN = `${process.env.HOME}/.claude/squeez/bin/squeez`;

export const SqueezPlugin = async () => {
  return {
    "tool.execute.before": async (input, output) => {
      if (input.tool === "bash") {
        const command = output.args?.command;
        if (!command || typeof command !== "string") return;
        if (command.startsWith(SQUEEZ_BIN)) return;
        if (command.includes("squeez wrap")) return;
        if (command.startsWith("--no-squeez")) return;
        output.args.command = `${SQUEEZ_BIN} wrap ${command}`;
      }
    },
  };
};
PLUGIN_EOF
}

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


# StatusLine — show squeez stats in Claude Code status bar
# Appends to existing statusLine command if claude-hud is present, otherwise sets standalone
existing_status = settings.get("statusLine", {})
existing_cmd = existing_status.get("command", "") if isinstance(existing_status, dict) else ""
squeez_status_cmd = "bash ~/.claude/squeez/bin/statusline.sh"
if "squeez" not in existing_cmd:
    if existing_cmd:
        # Append squeez after existing status (e.g., claude-hud)
        new_cmd = f"bash -c 'input=$(cat); echo \"$input\" | {{ {existing_cmd.rstrip()}; }} 2>/dev/null; echo \"$input\" | {squeez_status_cmd}'"
        settings["statusLine"] = {"type": "command", "command": new_cmd}
    else:
        settings["statusLine"] = {"type": "command", "command": squeez_status_cmd}

tmp = path + ".tmp"
with open(tmp, "w") as f:
    json.dump(settings, f, indent=2)
os.replace(tmp, path)
EOF

echo "Installing Copilot CLI hooks..."
COPILOT_SQUEEZ_DIR="$HOME/.copilot/squeez"
mkdir -p "$COPILOT_SQUEEZ_DIR/bin" "$COPILOT_SQUEEZ_DIR/hooks" \
         "$COPILOT_SQUEEZ_DIR/sessions" "$COPILOT_SQUEEZ_DIR/memory"
chmod 700 "$COPILOT_SQUEEZ_DIR" "$COPILOT_SQUEEZ_DIR/sessions" "$COPILOT_SQUEEZ_DIR/memory" 2>/dev/null || true

# Symlink the same binary so SQUEEZ_DIR-aware calls work
ln -sf "$INSTALL_DIR/bin/$BIN_NAME" "$COPILOT_SQUEEZ_DIR/bin/$BIN_NAME" 2>/dev/null || \
    cp "$INSTALL_DIR/bin/$BIN_NAME" "$COPILOT_SQUEEZ_DIR/bin/$BIN_NAME"

curl -fsSL "$REPO_RAW/hooks/copilot-pretooluse.sh"     -o "$INSTALL_DIR/hooks/copilot-pretooluse.sh"
curl -fsSL "$REPO_RAW/hooks/copilot-session-start.sh"  -o "$INSTALL_DIR/hooks/copilot-session-start.sh"
curl -fsSL "$REPO_RAW/hooks/copilot-posttooluse.sh"    -o "$INSTALL_DIR/hooks/copilot-posttooluse.sh"
chmod +x "$INSTALL_DIR/hooks/copilot-pretooluse.sh" \
         "$INSTALL_DIR/hooks/copilot-session-start.sh" \
         "$INSTALL_DIR/hooks/copilot-posttooluse.sh"

# Seed Copilot instructions (writes ~/.copilot/copilot-instructions.md)
"$INSTALL_DIR/bin/$BIN_NAME" init --copilot 2>/dev/null || true

# Register hooks in ~/.copilot/settings.json (Copilot CLI hook format mirrors Claude Code)
if [ -d "$HOME/.copilot" ]; then
python3 - <<'COPILOT_EOF'
import json, os, sys
path = os.path.expanduser("~/.copilot/settings.json")
settings = {}
try:
    if os.path.exists(path):
        with open(path) as f:
            settings = json.load(f)
except (json.JSONDecodeError, IOError) as e:
    print(f"Warning: could not read ~/.copilot/settings.json: {e}", file=sys.stderr)

if not isinstance(settings.get("PreToolUse"), list):
    settings["PreToolUse"] = []
pre = {"matcher": "Bash", "hooks": [{"type": "command", "command": "bash ~/.claude/squeez/hooks/copilot-pretooluse.sh"}]}
if not any("squeez" in str(h) for h in settings["PreToolUse"]):
    settings["PreToolUse"].append(pre)

if not isinstance(settings.get("SessionStart"), list):
    settings["SessionStart"] = []
start = {"hooks": [{"type": "command", "command": "bash ~/.claude/squeez/hooks/copilot-session-start.sh"}]}
if not any("squeez" in str(h) for h in settings["SessionStart"]):
    settings["SessionStart"].append(start)

if not isinstance(settings.get("PostToolUse"), list):
    settings["PostToolUse"] = []
post = {"hooks": [{"type": "command", "command": "bash ~/.claude/squeez/hooks/copilot-posttooluse.sh"}]}
if not any("squeez" in str(h) for h in settings["PostToolUse"]):
    settings["PostToolUse"].append(post)

tmp = path + ".tmp"
with open(tmp, "w") as f:
    json.dump(settings, f, indent=2)
os.replace(tmp, path)
COPILOT_EOF
fi

version=$("$INSTALL_DIR/bin/$BIN_NAME" --version 2>/dev/null || echo "squeez")
echo "✅ $version installed."
echo ""
echo "Claude Code:  Restart Claude Code to activate."
echo "OpenCode:     Restart OpenCode to activate the plugin (automatic Bash compression)."
echo "Copilot CLI:  Memory injected into ~/.copilot/copilot-instructions.md."
echo "              Restart Copilot CLI to activate hook-based bash compression."
