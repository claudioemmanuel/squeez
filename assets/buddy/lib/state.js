'use strict';

const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const { buildBuddy, rankIndexFromXp, rankFromIndex, applyDecay, daysBetween } = require('./engine');

const STATE_DIR = process.env.CLAUDE_BUDDY_HOME || path.join(os.homedir(), '.claude-buddy');
const STATE_FILE = path.join(STATE_DIR, 'state.json');

function todayIso() {
  return new Date().toISOString().slice(0, 10);
}

function ensureDir() {
  fs.mkdirSync(STATE_DIR, { recursive: true });
}

/** Identidade estável do usuário. MVP: hostname+usuário do SO como fallback --
 * trocar por accountUuid do Claude Code quando disponível no payload do hook. */
function resolveIdentity() {
  return process.env.CLAUDE_BUDDY_IDENTITY || `${os.hostname()}:${os.userInfo().username}`;
}

/** Cria o estado inicial (hatch) se ainda não existir. */
function hatchIfNeeded() {
  if (fs.existsSync(STATE_FILE)) return readState();

  const identity = resolveIdentity();
  const buddy = buildBuddy(identity);
  const state = {
    identity,
    name: 'Pato',
    stats: buddy.stats,
    peakName: buddy.peakName,
    dumpName: buddy.dumpName,
    archetype: buddy.archetype,
    intensity: buddy.intensity,
    xp: 0,
    shiny: (buddy.identity.length * 7) % 100 === 0, // placeholder simples de 1%
    createdAt: todayIso(),
    lastSessionDate: todayIso(),
    hasRecentError: false,
    lastComment: null,
  };
  ensureDir();
  fs.writeFileSync(STATE_FILE, JSON.stringify(state, null, 2));
  return state;
}

function readState() {
  ensureDir();
  if (!fs.existsSync(STATE_FILE)) return hatchIfNeeded();
  return JSON.parse(fs.readFileSync(STATE_FILE, 'utf8'));
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

/** Roda no SessionStart: aplica decaimento e atualiza a data da última sessão. */
function touchSession() {
  const state = readState();
  const idleDays = daysBetween(state.lastSessionDate, todayIso());
  const decayedIndex = applyDecay(state.xp, idleDays);
  state.effectiveRankIndex = decayedIndex;
  state.lastSessionDate = todayIso();
  return writeState(state);
}

/** Degrau efetivo (já considerando decaimento), pronto pra renderizar. */
function currentRank(state) {
  const idleDays = daysBetween(state.lastSessionDate, todayIso());
  const idx = applyDecay(state.xp, idleDays);
  return rankFromIndex(idx);
}

module.exports = {
  STATE_FILE,
  STATE_DIR,
  resolveIdentity,
  hatchIfNeeded,
  readState,
  writeState,
  addXp,
  touchSession,
  currentRank,
  todayIso,
};
