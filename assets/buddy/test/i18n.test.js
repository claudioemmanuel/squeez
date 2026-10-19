'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const {
  normalizeLang,
  archetypeId,
  intensityId,
  archetypeLabel,
  intensityLabel,
  rankLabel,
} = require('../lib/i18n');
const { ARCHETYPES, RANKS } = require('../lib/engine');
const { renderCard } = require('../lib/art');

const ROOT = path.join(__dirname, '..');
const SESSION_START = path.join(ROOT, 'hooks', 'session-start.js');
const CLI = path.join(ROOT, 'buddy-cli.js');

function todayIso() {
  return new Date().toISOString().slice(0, 10);
}

/** Isolated buddy home + squeez dir; `lang` null means no config.ini at all. */
function sandbox(t, lang, state) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'buddy-i18n-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const home = path.join(root, 'buddy');
  const squeez = path.join(root, 'squeez');
  fs.mkdirSync(home);
  fs.mkdirSync(squeez);
  if (lang !== null) fs.writeFileSync(path.join(squeez, 'config.ini'), `# cfg\nlang = ${lang}\n`);
  if (state) fs.writeFileSync(path.join(home, 'state.json'), JSON.stringify(state));
  return { home, squeez };
}

function run(script, args, box, input = '') {
  const result = spawnSync(process.execPath, [script, ...args], {
    input,
    env: { ...process.env, CLAUDE_BUDDY_HOME: box.home, SQUEEZ_DIR: box.squeez },
    encoding: 'utf8',
  });
  assert.strictEqual(result.status, 0, `${path.basename(script)} failed: ${result.stderr}`);
  return result.stdout;
}

function sessionContext(box) {
  const out = run(SESSION_START, [], box, JSON.stringify({ session_id: 's1', source: 'startup' }));
  return JSON.parse(out).hookSpecificOutput.additionalContext;
}

/** A schema-2 state file written by the Portuguese-only buddy (origin/main). */
function legacyPortugueseState() {
  return {
    schemaVersion: 2,
    identity: 'host:user',
    name: 'Pato',
    stats: { SNARK: 83, DEBUGGING: 6, WISDOM: 34, CHAOS: 47, PATIENCE: 34 },
    peakName: 'SNARK',
    dumpName: 'DEBUGGING',
    archetype: 'Reclamão',
    intensity: 'intenso',
    xp: RANKS[11].xpRequired, // Epic I
    shiny: false,
    createdAt: '2026-07-23',
    lastSessionDate: todayIso(),
    xpResetAt: '2026-07-28',
    xpResetReason: 'migration',
    lastTestRun: null,
    sessionKey: null,
    sessionXp: 0,
    sessionEditXp: 0,
    lastComment: null,
    squeez: { lastStartTs: 1, lastNetSaved: 0, lifetimeNetSaved: 137290, xpGranted: 5, rate: 25000 },
  };
}

test('normalizeLang mirrors the Rust Locale::from_code mapping', () => {
  for (const code of ['pt-BR', 'pt-br', 'pt', 'pt_BR']) assert.strictEqual(normalizeLang(code), 'pt-BR', code);
  for (const code of ['en', '', undefined, 'fr', 'PT-BR']) assert.strictEqual(normalizeLang(code), 'en', String(code));
});

test('labels from either language resolve to the same canonical id', () => {
  assert.strictEqual(archetypeId('Reclamão'), ARCHETYPES.RECLAMAO);
  assert.strictEqual(archetypeId('Grumbler'), ARCHETYPES.RECLAMAO);
  assert.strictEqual(archetypeId(ARCHETYPES.RECLAMAO), ARCHETYPES.RECLAMAO);
  assert.strictEqual(archetypeId('Fiscal/Guardião'), ARCHETYPES.FISCAL);
  assert.strictEqual(intensityId('intenso'), 'intense');
  assert.strictEqual(intensityId('intense'), 'intense');
  assert.strictEqual(archetypeLabel(ARCHETYPES.FISCAL, 'pt-BR'), 'Fiscal/Guardião');
  assert.strictEqual(archetypeLabel(ARCHETYPES.FISCAL, 'en'), 'Inspector/Guardian');
  assert.strictEqual(intensityLabel('moderate', 'pt-BR'), 'moderado');
  assert.strictEqual(rankLabel(RANKS[11], 'pt-BR'), 'Épico I');
  assert.strictEqual(rankLabel(RANKS[11], 'en'), 'Epic I');
});

test('card renders archetype in the requested language', () => {
  const args = { name: 'Pato', archetype: ARCHETYPES.RECLAMAO, rankHex: '#C084FC', rankIndex: 11, xp: 1, shiny: false, ansi: false };
  assert.ok(renderCard({ ...args, rankLabel: 'Épico I', lang: 'pt-BR' }).includes('Reclamão · Épico I'));
  assert.ok(renderCard({ ...args, rankLabel: 'Epic I', lang: 'en' }).includes('Grumbler · Epic I'));
});

test('lang = en renders the English SessionStart context', (t) => {
  const box = sandbox(t, 'en', legacyPortugueseState());
  const ctx = sessionContext(box);
  assert.ok(
    ctx.startsWith('You have a companion named Pato (archetype: Grumbler, intensity: intense, rank: Epic I).'),
    ctx
  );
  assert.ok(ctx.includes('Current card:\n'));
});

test('no config.ini defaults to English', (t) => {
  const ctx = sessionContext(sandbox(t, null, legacyPortugueseState()));
  assert.ok(ctx.startsWith('You have a companion named'), ctx);
});

test('lang = pt-BR renders the original Portuguese SessionStart context', (t) => {
  const box = sandbox(t, 'pt-BR', legacyPortugueseState());
  const ctx = sessionContext(box);
  assert.ok(
    ctx.startsWith(
      'Você tem um companheiro chamado Pato (arquétipo: Reclamão, intensidade: intenso, degrau: Épico I).'
    ),
    ctx
  );
  assert.ok(ctx.includes('Card atual:\n'));
  assert.ok(ctx.includes('Reclamão · Épico I'));
});

test('legacy Portuguese state keeps identity, XP and history under both languages', (t) => {
  for (const lang of ['pt-BR', 'en']) {
    const legacy = legacyPortugueseState();
    const box = sandbox(t, lang, legacy);
    sessionContext(box);
    const state = JSON.parse(fs.readFileSync(path.join(box.home, 'state.json'), 'utf8'));
    assert.strictEqual(state.archetype, ARCHETYPES.RECLAMAO, `${lang}: canonical archetype id persisted`);
    assert.strictEqual(state.intensity, 'intense', `${lang}: canonical intensity id persisted`);
    for (const key of ['identity', 'name', 'xp', 'createdAt', 'xpResetAt', 'xpResetReason']) {
      assert.strictEqual(state[key], legacy[key], `${lang}: ${key} preserved`);
    }
    assert.deepStrictEqual(state.stats, legacy.stats);
    assert.deepStrictEqual(state.squeez, legacy.squeez);
  }
});

test('CLI stats follows lang', (t) => {
  const pt = run(CLI, ['stats'], sandbox(t, 'pt-BR', legacyPortugueseState()));
  assert.ok(pt.startsWith('Pato — Reclamão · Épico I\n'), pt);
  assert.match(pt, /Ritmo: .* XP\/dia/);
  assert.match(pt, /Economia squeez acumulada: 137\.290 tokens líquidos/);

  const en = run(CLI, ['stats'], sandbox(t, 'en', legacyPortugueseState()));
  assert.ok(en.startsWith('Pato — Grumbler · Epic I\n'), en);
  assert.match(en, /Pace: .* XP\/day/);
  assert.match(en, /Accumulated squeez savings: 137,290 net tokens/);
});
