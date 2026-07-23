'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');

// Isola o data dir do squeez antes de carregar o módulo.
const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'buddy-squeez-test-'));
process.env.SQUEEZ_DIR = tmpDir;
const sessionsDir = path.join(tmpDir, 'sessions');
fs.mkdirSync(sessionsDir, { recursive: true });
const currentPath = path.join(sessionsDir, 'current.json');

const { applySqueezSavings, TOKENS_PER_XP } = require('../lib/squeez');

function writeSession(startTs, tokensSaved, overheadTokens) {
  fs.writeFileSync(
    currentPath,
    JSON.stringify({
      session_file: 'x.jsonl',
      total_tokens: 0,
      tokens_saved: tokensSaved,
      total_calls: 1,
      compact_warned: false,
      state_warned: false,
      start_ts: startTs,
      overhead_tokens: overheadTokens,
    })
  );
}

test('delta normal: economia líquida vira XP a 100:1', () => {
  const state = {};
  writeSession(1000, 550, 50); // net 500
  assert.strictEqual(applySqueezSavings(state), 500 / TOKENS_PER_XP);
  assert.strictEqual(state.squeez.lifetimeNetSaved, 500);
});

test('idempotente: sem economia nova, delta 0 (seguro PostToolUse+Stop)', () => {
  const state = {};
  writeSession(1000, 550, 50);
  applySqueezSavings(state);
  assert.strictEqual(applySqueezSavings(state), 0);
});

test('clamp: overhead maior que economia nunca dá XP negativo', () => {
  const state = {};
  writeSession(1000, 30, 200); // net clampado a 0
  assert.strictEqual(applySqueezSavings(state), 0);
  assert.strictEqual(state.squeez.lifetimeNetSaved, 0);
});

test('net regredindo (overhead cresce) não desconta do lifetime', () => {
  const state = {};
  writeSession(1000, 500, 0); // net 500
  applySqueezSavings(state);
  writeSession(1000, 500, 300); // net cai p/ 200
  assert.strictEqual(applySqueezSavings(state), 0);
  assert.strictEqual(state.squeez.lifetimeNetSaved, 500);
});

test('reset por start_ts: sessão nova zera baseline, lifetime acumula', () => {
  const state = {};
  writeSession(1000, 500, 0);
  applySqueezSavings(state); // +5 XP, lifetime 500
  writeSession(2000, 300, 0); // sessão nova, net 300
  assert.strictEqual(applySqueezSavings(state), 3);
  assert.strictEqual(state.squeez.lifetimeNetSaved, 800);
});

test('carry sub-100: resto acumula entre chamadas', () => {
  const state = {};
  writeSession(1000, 60, 0);
  assert.strictEqual(applySqueezSavings(state), 0); // 60 < 100
  writeSession(1000, 120, 0);
  assert.strictEqual(applySqueezSavings(state), 1); // lifetime 120 → 1 XP
  assert.strictEqual(state.squeez.xpGranted, 1);
});

test('current.json ausente: retorna 0, não lança', () => {
  fs.rmSync(currentPath, { force: true });
  assert.strictEqual(applySqueezSavings({}), 0);
});

test('current.json corrompido: retorna 0, não lança', () => {
  fs.writeFileSync(currentPath, '{not json');
  assert.strictEqual(applySqueezSavings({}), 0);
});

test('tracker corrompido no state é reconstruído', () => {
  const state = { squeez: 'garbage' };
  writeSession(3000, 250, 0);
  assert.strictEqual(applySqueezSavings(state), 2);
  assert.strictEqual(typeof state.squeez, 'object');
});
