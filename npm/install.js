#!/usr/bin/env node
'use strict';
/**
 * postinstall — downloads the squeez binary and registers Claude Code hooks.
 * Runs automatically on `npm install -g squeez` or `npx squeez`.
 */
const { execSync } = require('child_process');
const https = require('https');
const fs = require('fs');
const path = require('path');
const os = require('os');

// Allow skipping (e.g. in CI that only needs the npm package metadata)
if (process.env.SQUEEZ_SKIP_INSTALL === '1') process.exit(0);

const INSTALL_DIR = path.join(os.homedir(), '.claude', 'squeez');
const BINARY = path.join(INSTALL_DIR, 'bin', process.platform === 'win32' ? 'squeez.exe' : 'squeez');
const RELEASES = 'https://github.com/claudioemmanuel/squeez/releases/latest/download';

function log(msg) { process.stdout.write(`squeez: ${msg}\n`); }
function warn(msg) { process.stderr.write(`squeez: ${msg}\n`); }

// ── Platform map ───────────────────────────────────────────────────────────
const PLATFORM_MAP = {
  darwin:  { x64: 'squeez-macos-universal', arm64: 'squeez-macos-universal' },
  linux:   { x64: 'squeez-linux-x86_64', arm64: 'squeez-linux-aarch64' },
  win32:   { x64: 'squeez-windows-x86_64.exe' },
};

function getBinary() {
  const plat = PLATFORM_MAP[process.platform];
  if (!plat) return null;
  return plat[process.arch] || plat['x64'] || null;
}

// ── Download helper (Node built-in https, no external deps) ───────────────
function download(url, dest) {
  return new Promise((resolve, reject) => {
    const tmp = dest + '.tmp';
    const file = fs.createWriteStream(tmp);
    const follow = (u) => {
      https.get(u, (res) => {
        if (res.statusCode === 301 || res.statusCode === 302) {
          follow(res.headers.location);
          return;
        }
        if (res.statusCode !== 200) {
          reject(new Error(`HTTP ${res.statusCode} for ${u}`));
          return;
        }
        res.pipe(file);
        file.on('finish', () => {
          file.close();
          fs.renameSync(tmp, dest);
          resolve();
        });
      }).on('error', reject);
    };
    follow(url);
  });
}

async function main() {
  // Already installed and up-to-date — just register hooks
  if (fs.existsSync(BINARY)) {
    log('binary already present — skipping download.');
    registerHooks();
    return;
  }

  // ── Unix: prefer install.sh (also handles hooks + Copilot) ───────────
  if (process.platform !== 'win32') {
    try {
      log('running install.sh …');
      execSync(
        'curl -fsSL https://raw.githubusercontent.com/claudioemmanuel/squeez/main/install.sh | sh',
        { stdio: 'inherit' }
      );
      return;
    } catch (_) {
      warn('curl not found — falling back to Node.js downloader.');
    }
  }

  // ── Fallback: Node.js binary download (no curl required) ─────────────
  const binaryName = getBinary();
  if (!binaryName) {
    warn(`unsupported platform ${process.platform}/${process.arch}.`);
    warn('Download manually from https://github.com/claudioemmanuel/squeez/releases');
    process.exit(0);
  }

  fs.mkdirSync(path.join(INSTALL_DIR, 'bin'), { recursive: true });
  fs.mkdirSync(path.join(INSTALL_DIR, 'hooks'), { recursive: true });
  fs.mkdirSync(path.join(INSTALL_DIR, 'sessions'), { recursive: true });

  log(`downloading ${binaryName} …`);
  try {
    await download(`${RELEASES}/${binaryName}`, BINARY);
    fs.chmodSync(BINARY, 0o755);
    log('binary downloaded.');
  } catch (err) {
    warn(`download failed: ${err.message}`);
    warn('Install manually: curl -fsSL https://raw.githubusercontent.com/claudioemmanuel/squeez/main/install.sh | sh');
    process.exit(0);
  }

  registerHooks();
}

function registerHooks() {
  const settingsPath = path.join(os.homedir(), '.claude', 'settings.json');
  let settings = {};
  const fileExisted = fs.existsSync(settingsPath);
  if (fileExisted) {
    let raw;
    try {
      raw = fs.readFileSync(settingsPath, 'utf8');
      if (raw.charCodeAt(0) === 0xFEFF) raw = raw.slice(1); // strip BOM
    } catch (err) {
      warn(`could not read ${settingsPath}: ${err.message} — skipping hook registration.`);
      return;
    }
    try {
      settings = JSON.parse(raw);
    } catch (err) {
      warn(`refusing to overwrite ${settingsPath}: could not parse existing JSON (${err.message}).`);
      warn(`fix or remove the file, then re-run: squeez setup`);
      return;
    }
    if (typeof settings !== 'object' || settings === null || Array.isArray(settings)) {
      warn(`refusing to overwrite ${settingsPath}: top-level value is not a JSON object.`);
      return;
    }
  }

  let changed = false;

  // PreToolUse
  if (!Array.isArray(settings.PreToolUse)) { settings.PreToolUse = []; }
  const preHook = { matcher: 'Bash', hooks: [{ type: 'command', command: 'bash ~/.claude/squeez/hooks/pretooluse.sh' }] };
  if (!settings.PreToolUse.some(h => JSON.stringify(h).includes('squeez'))) {
    settings.PreToolUse.push(preHook); changed = true;
  }

  // SessionStart
  if (!Array.isArray(settings.SessionStart)) { settings.SessionStart = []; }
  const startHook = { hooks: [{ type: 'command', command: 'bash ~/.claude/squeez/hooks/session-start.sh' }] };
  if (!settings.SessionStart.some(h => JSON.stringify(h).includes('squeez'))) {
    settings.SessionStart.push(startHook); changed = true;
  }

  // PostToolUse
  if (!Array.isArray(settings.PostToolUse)) { settings.PostToolUse = []; }
  const postHook = { hooks: [{ type: 'command', command: 'bash ~/.claude/squeez/hooks/posttooluse.sh' }] };
  if (!settings.PostToolUse.some(h => JSON.stringify(h).includes('squeez'))) {
    settings.PostToolUse.push(postHook); changed = true;
  }

  // StatusLine
  const squeezStatusCmd = 'bash ~/.claude/squeez/bin/statusline.sh';
  const existingStatus = typeof settings.statusLine === 'object' ? settings.statusLine : {};
  const existingCmd = existingStatus.command || '';
  if (!existingCmd.includes('squeez')) {
    settings.statusLine = existingCmd
      ? { type: 'command', command: `bash -c 'input=$(cat); echo "$input" | { ${existingCmd}; } 2>/dev/null; echo "$input" | ${squeezStatusCmd}'` }
      : { type: 'command', command: squeezStatusCmd };
    changed = true;
  }

  if (changed) {
    try {
      if (fileExisted) {
        try { fs.copyFileSync(settingsPath, settingsPath + '.bak'); } catch (_) {}
      }
      fs.writeFileSync(settingsPath + '.tmp', JSON.stringify(settings, null, 2));
      fs.renameSync(settingsPath + '.tmp', settingsPath);
      log('Claude Code hooks registered.');
    } catch (err) {
      warn(`could not write settings.json: ${err.message}`);
    }
  }
}

main().catch(err => { warn(err.message); process.exit(0); });
