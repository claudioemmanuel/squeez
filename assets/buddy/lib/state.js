'use strict';

const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const { buildBuddy, rankIndexFromXp, rankFromIndex, applyDecay, daysBetween } = require('./engine');

const STATE_DIR = process.env.CLAUDE_BUDDY_HOME || path.join(os.homedir(), '.claude-buddy');
const STATE_FILE = path.join(STATE_DIR, 'state.json');

// v2: reset de XP. O placar acumulado até aqui foi produzido em cima de um
// gatilho quebrado (ver hooks/post-tool-use.js) e não representa progressão
// nenhuma -- o primeiro pato em produção fez 3.563 XP em 5 dias, 10x o que o
// design assumia. Migrar zera o placar UMA vez; identidade, stats, arquétipo e
// nome não são tocados: é o mesmo pato, só o contador volta ao zero.
const SCHEMA_VERSION = 2;

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
    schemaVersion: SCHEMA_VERSION,
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
    lastTestRun: null,
    sessionKey: null,
    sessionXp: 0,
    sessionEditXp: 0,
    // Gancho da rodada de voz: hooks/stop.js preenche isto a partir de um
    // `<!-- buddy: ... -->` na resposta do modelo e a status line desenha como
    // balão. Hoje nada instrui o modelo a emitir esse comentário, então fica
    // sempre null -- o balão só liga quando a biblioteca de falas entrar.
    lastComment: null,
  };
  ensureDir();
  fs.writeFileSync(STATE_FILE, JSON.stringify(state, null, 2));
  return state;
}

/**
 * Zera o placar mantendo a identidade do pato. `reason` só entra no state pra
 * ficar auditável depois ('migration' | 'manual').
 */
function resetXp(state, reason) {
  state.xp = 0;
  state.xpResetAt = todayIso();
  state.xpResetReason = reason;
  if (state.squeez && typeof state.squeez === 'object') {
    // O lifetime economizado é histórico real e fica; só o já-concedido zera,
    // senão a economia passada recompraria o XP na primeira chamada.
    state.squeez.xpGranted = 0;
  }
  state.sessionKey = null;
  state.sessionXp = 0;
  state.sessionEditXp = 0;
  state.schemaVersion = SCHEMA_VERSION;
  return state;
}

function readState() {
  ensureDir();
  if (!fs.existsSync(STATE_FILE)) return hatchIfNeeded();
  const state = JSON.parse(fs.readFileSync(STATE_FILE, 'utf8'));
  if ((state.schemaVersion || 1) < SCHEMA_VERSION) {
    delete state.hasRecentError; // gatilho removido, ver hooks/post-tool-use.js
    return writeState(resetXp(state, 'migration'));
  }
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
 * Chave da sessão para o teto de XP de ação. Usa o session id do host quando
 * ele vem no payload do hook; sem isso, o dia corrente já limita o grind.
 */
function sessionKeyFrom(input) {
  const id = input && (input.session_id || input.sessionId);
  return typeof id === 'string' && id ? id : todayIso();
}

/** Roda no SessionStart: aplica decaimento e atualiza a data da última sessão. */
function touchSession() {
  const state = readState();
  const idleDays = daysBetween(state.lastSessionDate, todayIso());
  const decayedIndex = applyDecay(state.xp, idleDays);
  state.effectiveRankIndex = decayedIndex;
  state.lastSessionDate = todayIso();
  state.sessionKey = null; // sessão nova: o teto de XP de ação recomeça
  state.sessionXp = 0;
  state.sessionEditXp = 0;
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
