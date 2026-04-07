#!/usr/bin/env node
'use strict';
/**
 * bin.js — CLI entry point for the squeez npm package.
 * Delegates all arguments to the native binary at ~/.claude/squeez/bin/squeez.
 * Auto-installs if the binary is missing (e.g. after `npx squeez`).
 */
const { execFileSync, execSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');

const BINARY = path.join(
  os.homedir(), '.claude', 'squeez', 'bin',
  process.platform === 'win32' ? 'squeez.exe' : 'squeez'
);

if (!fs.existsSync(BINARY)) {
  process.stderr.write('squeez: binary not found — running installer...\n');
  if (process.platform === 'win32') {
    process.stderr.write('squeez: on Windows, download from https://github.com/claudioemmanuel/squeez/releases\n');
    process.exit(1);
  }
  try {
    execSync(
      'curl -fsSL https://raw.githubusercontent.com/claudioemmanuel/squeez/main/install.sh | sh',
      { stdio: 'inherit' }
    );
  } catch (_) {
    // If curl failed, try running install.js alongside this file
    try {
      require(path.join(__dirname, 'install.js'));
    } catch (e) {
      process.stderr.write(`squeez: installation failed: ${e.message}\n`);
      process.exit(1);
    }
  }
}

if (!fs.existsSync(BINARY)) {
  process.stderr.write('squeez: binary still not found after install attempt.\n');
  process.stderr.write('Run manually: curl -fsSL https://raw.githubusercontent.com/claudioemmanuel/squeez/main/install.sh | sh\n');
  process.exit(1);
}

try {
  execFileSync(BINARY, process.argv.slice(2), { stdio: 'inherit' });
} catch (e) {
  process.exit(e.status ?? 1);
}
