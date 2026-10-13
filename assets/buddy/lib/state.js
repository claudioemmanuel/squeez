'use strict';

const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const { buildBuddy, rankIndexFromXp, rankFromIndex, applyDecay, daysBetween } = require('./engine');

const STATE_DIR = process.env.CLAUDE_BUDDY_HOME || path.join(os.homedir(), '.claude-buddy');
const STATE_FILE = path.join(STATE_DIR, 'state.json');

// v2: XP reset. The score accumulated so far was produced on top of a broken
// trigger (see hooks/post-tool-use.js) and does not represent any real
// progression -- the first duck in production made 3,563 XP in 5 days, 10x what
// the design assumed. Migrating zeroes the score ONCE; identity, stats,
// archetype and name are untouched: it is the same duck, only the counter goes
// back to zero.
const SCHEMA_VERSION = 2;

function todayIso() {
  return new Date().toISOString().slice(0, 10);
}

function ensureDir() {
  fs.mkdirSync(STATE_DIR, { recursive: true });
}

/** Stable identity for the user. MVP: OS hostname+user as fallback --
 * swap for the Claude Code accountUuid once it is available in the hook payload. */
function resolveIdentity() {
  return process.env.CLAUDE_BUDDY_IDENTITY || `${os.hostname()}:${os.userInfo().username}`;
}

/** Create the initial state (hatch) if it doesn't exist yet. */
function hatchIfNeeded() {
  if (fs.existsSync(STATE_FILE)) return readState();

  const identity = resolveIdentity();
  const buddy = buildBuddy(identity);
  const state = {
    schemaVersion: SCHEMA_VERSION,
    identity,
    name: 'Pato',
    stats: buddy.stats,
    peakName: buddy.peakName,
    dumpName: buddy.dumpName,
    archetype: buddy.archetype,
    intensity: buddy.intensity,
    xp: 0,
    shiny: (buddy.identity.length * 7) % 100 === 0, // simple 1% placeholder
    createdAt: todayIso(),
    lastSessionDate: todayIso(),
    lastTestRun: null,
    sessionKey: null,
    sessionXp: 0,
    sessionEditXp: 0,
    // Voice-round hook: hooks/stop.js fills this from a `<!-- buddy: ... -->`
    // in the model's reply and the status line draws it as a bubble. Today
    // nothing instructs the model to emit that comment, so it is always null --
    // the bubble only turns on once the speech library lands.
    lastComment: null,
  };
  ensureDir();
  fs.writeFileSync(STATE_FILE, JSON.stringify(state, null, 2));
  return state;
}

/**
 * Zeroes the score while keeping the duck's identity. `reason` only goes into
 * the state so it stays auditable later ('migration' | 'manual').
 */
function resetXp(state, reason) {
  state.xp = 0;
  state.xpResetAt = todayIso();
  state.xpResetReason = reason;
  if (state.squeez && typeof state.squeez === 'object') {
    // The lifetime savings are real history and stay; only what was already
    // granted is zeroed, otherwise past savings would buy the XP back on the
    // first call.
    state.squeez.xpGranted = 0;
  }
  state.sessionKey = null;
  state.sessionXp = 0;
  state.sessionEditXp = 0;
  state.schemaVersion = SCHEMA_VERSION;
  return state;
}

// Portuguese text values persisted by pre-translation state files. These are
// legacy DATA keys, not user-facing text: mapping them keeps an existing duck's
// identity intact while every label renders in English.
const LEGACY_ARCHETYPES = {
  'Mandão': 'Bossy',
  'Reclamão': 'Grumbler',
  'Caótico': 'Chaotic',
  'Fiscal/Guardião': 'Inspector/Guardian',
  'Sábio': 'Sage',
  'Motivador': 'Motivator',
};
const LEGACY_INTENSITIES = { leve: 'mild', moderado: 'moderate', intenso: 'intense' };

/** Translates legacy Portuguese text values in place; true when anything changed. */
function migrateLegacyText(state) {
  let changed = false;
  if (LEGACY_ARCHETYPES[state.archetype]) {
    state.archetype = LEGACY_ARCHETYPES[state.archetype];
    changed = true;
  }
  if (LEGACY_INTENSITIES[state.intensity]) {
    state.intensity = LEGACY_INTENSITIES[state.intensity];
    changed = true;
  }
  return changed;
}

function readState() {
  ensureDir();
  if (!fs.existsSync(STATE_FILE)) return hatchIfNeeded();
  const state = JSON.parse(fs.readFileSync(STATE_FILE, 'utf8'));
  if ((state.schemaVersion || 1) < SCHEMA_VERSION) {
    delete state.hasRecentError; // trigger removed, see hooks/post-tool-use.js
    resetXp(state, 'migration');
    migrateLegacyText(state);
    return writeState(state);
  }
  // Legacy (pre-translation) state files carry Portuguese archetype/intensity
  // values; translate them in place once so the same duck renders in English.
  if (migrateLegacyText(state)) return writeState(state);
  return state;
}

function writeState(state) {
  ensureDir();
  fs.writeFileSync(STATE_FILE, JSON.stringify(state, null, 2));
  return state;
}

function addXp(amount) {
  const state = readState();
  state.xp = Math.max(0, state.xp + amount);
  return writeState(state);
}

/**
 * Session key for the action XP cap. Uses the host session id when it comes in
 * the hook payload; without it, the current day already limits the grind.
 */
function sessionKeyFrom(input) {
  const id = input && (input.session_id || input.sessionId);
  return typeof id === 'string' && id ? id : todayIso();
}

/** Runs on SessionStart: applies decay and updates the last-session date. */
function touchSession() {
  const state = readState();
  const idleDays = daysBetween(state.lastSessionDate, todayIso());
  const decayedIndex = applyDecay(state.xp, idleDays);
  state.effectiveRankIndex = decayedIndex;
  state.lastSessionDate = todayIso();
  state.sessionKey = null; // new session: the action XP cap restarts
  state.sessionXp = 0;
  state.sessionEditXp = 0;
  return writeState(state);
}

/** Effective rank (already accounting for decay), ready to render. */
function currentRank(state) {
  const idleDays = daysBetween(state.lastSessionDate, todayIso());
  const idx = applyDecay(state.xp, idleDays);
  return rankFromIndex(idx);
}

module.exports = {
  STATE_FILE,
  STATE_DIR,
  SCHEMA_VERSION,
  resolveIdentity,
  hatchIfNeeded,
  readState,
  writeState,
  addXp,
  resetXp,
  sessionKeyFrom,
  touchSession,
  currentRank,
  todayIso,
};
