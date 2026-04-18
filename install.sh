#!/usr/bin/env bash
# squeez installer — downloads the release binary and delegates all host
# registration to `squeez setup`, which iterates the HostAdapter registry
# (Claude Code, Copilot CLI, OpenCode, Gemini CLI, Codex CLI).
#
# Flags:
#   --setup-only   Skip binary download; use an existing `squeez` from PATH
#                  (for cargo-install / cargo-update users).
set -euo pipefail

RELEASES="https://github.com/claudioemmanuel/squeez/releases/latest/download"
INSTALL_DIR="$HOME/.claude/squeez"

SETUP_ONLY=0
for arg in "$@"; do
  case "$arg" in
    --setup-only) SETUP_ONLY=1 ;;
  esac
done

# ── Detect OS and architecture ─────────────────────────────────────────
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

case "$OS" in
  Windows*|MINGW*|MSYS*|CYGWIN*) BIN_NAME="squeez.exe" ;;
  *) BIN_NAME="squeez" ;;
esac

mkdir -p "$INSTALL_DIR/bin"
chmod 700 "$INSTALL_DIR" 2>/dev/null || true

# ── Stage 1: obtain the binary ─────────────────────────────────────────
if [ "$SETUP_ONLY" -eq 1 ]; then
  echo "Setup-only mode: using existing squeez from PATH..."
  EXISTING=$(command -v squeez 2>/dev/null || true)
  if [ -z "$EXISTING" ]; then
    echo "ERROR: squeez not found in PATH." >&2
    echo "Install first with: cargo install squeez" >&2
    exit 1
  fi
  echo "Found squeez at: $EXISTING"
  cp "$EXISTING" "$INSTALL_DIR/bin/$BIN_NAME"
  chmod +x "$INSTALL_DIR/bin/$BIN_NAME" 2>/dev/null || true
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

# ── Stage 2: delegate host registration to `squeez setup` ──────────────
# The Rust binary owns every host-specific concern now: config.ini writing,
# hook-script emission, settings.json patching, AGENTS.md / GEMINI.md /
# CLAUDE.md / copilot-instructions.md injection. install.sh is just the
# bootstrap.
echo ""
"$INSTALL_DIR/bin/$BIN_NAME" setup

version=$("$INSTALL_DIR/bin/$BIN_NAME" --version 2>/dev/null || echo "squeez")
echo ""
echo "✅ $version installed. Restart your CLI (Claude Code / Copilot CLI / OpenCode / Gemini / Codex) to activate."
