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

test('taxa é 100k tokens por XP (progressão lenta de propósito)', () => {
  assert.strictEqual(TOKENS_PER_XP, 100000);
});

test('delta normal: economia líquida vira XP na taxa vigente', () => {
  const state = {};
  writeSession(1000, 550000, 50000); // net 500k
  assert.strictEqual(applySqueezSavings(state), 500000 / TOKENS_PER_XP);
  assert.strictEqual(state.squeez.lifetimeNetSaved, 500000);
});

test('idempotente: sem economia nova, delta 0 (seguro PostToolUse+Stop)', () => {
  const state = {};
  writeSession(1000, 550000, 50000);
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
  writeSession(1000, 500000, 0); // net 500k
  applySqueezSavings(state);
  writeSession(1000, 500000, 300000); // net cai p/ 200k
  assert.strictEqual(applySqueezSavings(state), 0);
  assert.strictEqual(state.squeez.lifetimeNetSaved, 500000);
});

test('reset por start_ts: sessão nova zera baseline, lifetime acumula', () => {
  const state = {};
  writeSession(1000, 500000, 0);
  applySqueezSavings(state); // +5 XP, lifetime 500k
  writeSession(2000, 300000, 0); // sessão nova, net 300k
  assert.strictEqual(applySqueezSavings(state), 3);
  assert.strictEqual(state.squeez.lifetimeNetSaved, 800000);
});

test('carry sub-taxa: resto acumula entre chamadas', () => {
  const state = {};
  writeSession(1000, 60000, 0);
  assert.strictEqual(applySqueezSavings(state), 0); // 60k < 100k
  writeSession(1000, 120000, 0);
  assert.strictEqual(applySqueezSavings(state), 1); // lifetime 120k → 1 XP
  assert.strictEqual(state.squeez.xpGranted, 1);
});

test('re-base: tracker de taxa antiga (100:1) desconta o excesso concedido', () => {
  // Cenário real da migração: 19.184 tokens líquidos renderam 191 XP na taxa
  // antiga. Na taxa nova valem 0 XP → devolve -191 uma única vez.
  const state = {
    squeez: { lastStartTs: 1000, lastNetSaved: 19184, lifetimeNetSaved: 19184, xpGranted: 191 },
  };
  writeSession(1000, 20069, 885); // net 19184, sem economia nova
  assert.strictEqual(applySqueezSavings(state), -191);
  assert.strictEqual(state.squeez.xpGranted, 0);
  assert.strictEqual(state.squeez.rate, TOKENS_PER_XP);
  // Chamada seguinte é neutra.
  assert.strictEqual(applySqueezSavings(state), 0);
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
  writeSession(3000, 250000, 0);
  assert.strictEqual(applySqueezSavings(state), 2);
  assert.strictEqual(typeof state.squeez, 'object');
});
