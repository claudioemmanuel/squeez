'use strict';

const test = require('node:test');
const assert = require('node:assert');

const {
  ARCHETYPES,
  RANKS,
  RANK_COLORS,
  SUB_LEVELS,
  MAX_RANK_INDEX,
  FLOOR_RANK_INDEX,
  GRACE_PERIOD_DAYS,
  DECAY_INTERVAL_DAYS,
  STAT_NAMES,
  buildBuddy,
  generateStats,
  resolveArchetype,
  resolveIntensity,
  rankIndexFromXp,
  rankFromIndex,
  stageFromRank,
  applyDecay,
  daysBetween,
} = require('../lib/engine');

// ---------------------------------------------------------------------------
// Curva de XP
// ---------------------------------------------------------------------------

test('curva tem 15 degraus e é estritamente crescente', () => {
  assert.strictEqual(RANKS.length, RANK_COLORS.length * SUB_LEVELS.length);
  assert.strictEqual(RANKS.length, 15);
  for (let i = 1; i < RANKS.length; i++) {
    assert.ok(
      RANKS[i].xpRequired > RANKS[i - 1].xpRequired,
      `degrau ${i} (${RANKS[i].label}) precisa custar mais que o anterior`
    );
  }
  assert.strictEqual(RANKS[0].xpRequired, 0);
});

test('curva é geométrica: cada degrau custa mais que o anterior', () => {
  // Sem isso a curva vira quase-linear e dá pra varrer os degraus em dias --
  // exatamente o que quebrou a versão anterior.
  const cost = (i) => RANKS[i].xpRequired - RANKS[i - 1].xpRequired;
  for (let i = 2; i < RANKS.length; i++) {
    assert.ok(cost(i) > cost(i - 1), `custo do degrau ${i} não cresceu`);
  }
});

test('rankIndexFromXp acerta as fronteiras de cada degrau', () => {
  for (let i = 0; i < RANKS.length; i++) {
    const need = RANKS[i].xpRequired;
    assert.strictEqual(rankIndexFromXp(need), i, `${RANKS[i].label} no limiar exato`);
    if (i > 0) {
      assert.strictEqual(rankIndexFromXp(need - 1), i - 1, `${RANKS[i].label} 1 XP antes`);
    }
  }
  assert.strictEqual(rankIndexFromXp(0), 0);
  assert.strictEqual(rankIndexFromXp(-5), 0, 'XP negativo não quebra');
  assert.strictEqual(rankIndexFromXp(999_999_999), MAX_RANK_INDEX, 'satura no topo');
});

test('rankFromIndex satura nas pontas', () => {
  assert.strictEqual(rankFromIndex(-3).label, 'Comum III');
  assert.strictEqual(rankFromIndex(999).label, 'Lendário I');
});

test('rótulos vão de Comum III a Lendário I, III é o mais baixo da cor', () => {
  assert.strictEqual(RANKS[0].label, 'Comum III');
  assert.strictEqual(RANKS[MAX_RANK_INDEX].label, 'Lendário I');
  assert.strictEqual(RANKS[1].label, 'Comum II');
  assert.ok(RANKS[3].xpRequired > RANKS[2].xpRequired, 'Incomum III custa mais que Comum I');
});

// ---------------------------------------------------------------------------
// Estágio do desenho
// ---------------------------------------------------------------------------

test('stageFromRank: um estágio por cor de raridade', () => {
  const perColor = {};
  for (const rank of RANKS) {
    const stage = stageFromRank(rank.index);
    if (perColor[rank.color] === undefined) perColor[rank.color] = stage;
    assert.strictEqual(
      stage,
      perColor[rank.color],
      `${rank.label}: sub-nível não pode mudar o estágio`
    );
  }
  assert.deepStrictEqual(RANK_COLORS.map((c) => perColor[c]), [0, 1, 2, 3, 4]);
});

test('stageFromRank satura fora da faixa', () => {
  assert.strictEqual(stageFromRank(-1), 0);
  assert.strictEqual(stageFromRank(999), 4);
});

// ---------------------------------------------------------------------------
// Decaimento
// ---------------------------------------------------------------------------

test('decaimento respeita a carência e desce 1 sub-degrau por intervalo', () => {
  const xp = RANKS[6].xpRequired; // Raro III
  assert.strictEqual(applyDecay(xp, 0), 6);
  assert.strictEqual(applyDecay(xp, GRACE_PERIOD_DAYS), 6, 'dentro da carência não decai');
  assert.strictEqual(applyDecay(xp, GRACE_PERIOD_DAYS + DECAY_INTERVAL_DAYS), 5);
  assert.strictEqual(applyDecay(xp, GRACE_PERIOD_DAYS + DECAY_INTERVAL_DAYS * 2), 4);
});

test('decaimento nunca passa do piso', () => {
  assert.strictEqual(applyDecay(RANKS[14].xpRequired, 3650), FLOOR_RANK_INDEX);
});

test('daysBetween nunca é negativo', () => {
  assert.strictEqual(daysBetween('2026-07-10', '2026-07-13'), 3);
  assert.strictEqual(daysBetween('2026-07-13', '2026-07-10'), 0);
});

// ---------------------------------------------------------------------------
// Identidade determinística
// ---------------------------------------------------------------------------

test('mesma identidade gera sempre o mesmo pato', () => {
  const a = buildBuddy('host:user');
  const b = buildBuddy('host:user');
  assert.deepStrictEqual(a, b);
  assert.notDeepStrictEqual(buildBuddy('host:outro').stats, a.stats);
});

test('sempre existe exatamente 1 pico e 1 vale, e o pico é o maior', () => {
  for (let i = 0; i < 200; i++) {
    const { stats, peakName, dumpName } = generateStats(`ident-${i}`);
    const values = STAT_NAMES.map((n) => stats[n]);
    assert.strictEqual(stats[peakName], Math.max(...values), `${i}: pico é o maior`);
    assert.strictEqual(stats[dumpName], Math.min(...values), `${i}: vale é o menor`);
    assert.notStrictEqual(peakName, dumpName);
  }
});

test('árvore de arquétipos cobre as 20 combinações de pico/vale', () => {
  const known = new Set(Object.values(ARCHETYPES));
  let combos = 0;
  for (const peak of STAT_NAMES) {
    for (const dump of STAT_NAMES) {
      if (peak === dump) continue;
      combos++;
      assert.ok(known.has(resolveArchetype(peak, dump)), `${peak}/${dump} sem arquétipo`);
    }
  }
  assert.strictEqual(combos, 20);
});

test('intensidade vem da distância pico-vale, não do valor absoluto', () => {
  const stats = { A: 100, B: 80 };
  assert.strictEqual(resolveIntensity(stats, 'A', 'B'), 'leve'); // gap 20
  assert.strictEqual(resolveIntensity({ A: 60, B: 20 }, 'A', 'B'), 'moderado'); // gap 40
  assert.strictEqual(resolveIntensity({ A: 90, B: 10 }, 'A', 'B'), 'intenso'); // gap 80
});
