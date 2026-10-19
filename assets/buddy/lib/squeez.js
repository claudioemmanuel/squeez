'use strict';

// XP source from squeez savings: reads <squeez data dir>/sessions/current.json
// and converts net savings (tokens_saved - overhead_tokens) into XP.
//
// This is the PRIMARY XP source, and the only non-gameable one in the project:
// either squeez saved real tokens, or it didn't. That is why it has no
// per-session cap, unlike action XP (hooks/post-tool-use.js).
//
// RATE: 25,000 net tokens = 1 XP. The previous rate (1,000,000:1) was so high
// it sat inert -- real measurement from the first duck in production: 137,290
// tokens saved in 5 days yielded 0 XP, while a broken "bug fixed" trigger
// poured out ~700 XP/day. The honest faucet has to be the one that actually
// fills. At 25k:1, a heavy day with compression on (~300k net tokens) yields
// ~12 XP -- the premise lib/engine.js calibrates the curve on. This is the
// pace calibration knob; the curve is the shape.
//
// Baseline per start_ts: a new session resets the ruler; the sub-rate remainder
// stays accumulated in lifetimeNetSaved (nothing is lost). A rate change
// re-bases the XP already granted (tracker.rate records the current rate; the
// recomputation delta can be negative).

const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');

const TOKENS_PER_XP = 25000;

function squeezDataDir() {
  return process.env.SQUEEZ_DIR || path.join(os.homedir(), '.claude', 'squeez');
}

/**
 * Raw value of `key` in config.ini, or null when absent. Same semantics as the
 * Rust parser (`Config::from_str`): skips empty lines and `#`, cuts at the
 * first `=`, trims both sides.
 */
function configValue(key) {
  try {
    const raw = fs.readFileSync(path.join(squeezDataDir(), 'config.ini'), 'utf8');
    for (const line of raw.split('\n')) {
      const s = line.trim();
      if (!s || s.startsWith('#')) continue;
      const eq = s.indexOf('=');
      if (eq < 0) continue;
      if (s.slice(0, eq).trim() !== key) continue;
      return s.slice(eq + 1).trim();
    }
  } catch {
    /* no config.ini */
  }
  return null;
}

/** `context_window_tokens` from config.ini, or 0 when not pinned. */
function configuredContextWindow() {
  return Number(configValue('context_window_tokens')) || 0;
}

/** `lang` from config.ini, raw (lib/i18n.js normalizes it), or null. */
function configuredLang() {
  return configValue('lang');
}

function readCurrentSession() {
  try {
    const raw = fs.readFileSync(
      path.join(squeezDataDir(), 'sessions', 'current.json'),
      'utf8'
    );
    const j = JSON.parse(raw);
    if (!j || typeof j !== 'object') return null;
    return {
      startTs: Number(j.start_ts) || 0,
      tokensSaved: Number(j.tokens_saved) || 0,
      overheadTokens: Number(j.overhead_tokens) || 0,
    };
  } catch {
    return null;
  }
}

/**
 * Mutates state.squeez (tracker) and returns the XP to grant now. Normally an
 * integer >= 0; negative only on the re-base after a rate change (the caller
 * should still apply it via addXp). Idempotent across calls with no new
 * savings (monotonic baseline), so it is safe to call from both PostToolUse
 * and Stop.
 */
function applySqueezSavings(state) {
  const cur = readCurrentSession();
  if (!cur) return 0;

  const tracker =
    state.squeez && typeof state.squeez === 'object'
      ? state.squeez
      : { lastStartTs: 0, lastNetSaved: 0, lifetimeNetSaved: 0, xpGranted: 0, rate: TOKENS_PER_XP };
  state.squeez = tracker;

  // Re-base after a rate change: recompute the total XP the lifetime is worth
  // at the new rate and return the difference against what was already granted
  // (may be negative).
  let rebase = 0;
  if ((Number(tracker.rate) || 0) !== TOKENS_PER_XP) {
    const worth = Math.floor((Number(tracker.lifetimeNetSaved) || 0) / TOKENS_PER_XP);
    rebase = worth - (Number(tracker.xpGranted) || 0);
    tracker.xpGranted = worth;
    tracker.rate = TOKENS_PER_XP;
  }

  const net = Math.max(0, cur.tokensSaved - cur.overheadTokens);
  if (cur.startTs !== tracker.lastStartTs) {
    tracker.lastStartTs = cur.startTs;
    tracker.lastNetSaved = 0;
  }
  const delta = Math.max(0, net - (Number(tracker.lastNetSaved) || 0));
  tracker.lastNetSaved = Math.max(Number(tracker.lastNetSaved) || 0, net);
  tracker.lifetimeNetSaved = (Number(tracker.lifetimeNetSaved) || 0) + delta;

  const totalXp = Math.floor(tracker.lifetimeNetSaved / TOKENS_PER_XP);
  const granted = Number(tracker.xpGranted) || 0;
  const gain = Math.max(0, totalXp - granted);
  tracker.xpGranted = granted + gain;
  return rebase + gain;
}

module.exports = { applySqueezSavings, configuredContextWindow, configuredLang, TOKENS_PER_XP };
