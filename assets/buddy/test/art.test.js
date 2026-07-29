'use strict';

const test = require('node:test');
const assert = require('node:assert');

const { SLOT_KITS, STAGES, SIDE_WIDTH, fillBody, renderCard, renderStatusline } = require('../lib/art');
const { ARCHETYPES, RANKS, stageFromRank } = require('../lib/engine');

const ARCHETYPE_LIST = Object.values(ARCHETYPES);

// Glifos não-ASCII liberados no desenho, cada um verificado como largura de
// exibição 1. `★` (U+2605), `⊙` (U+2299) e `▤` (U+25A4) NÃO entram: são East
// Asian Ambiguous/Wide e ocupam 2 colunas em vários terminais, o que desalinhava
// o card e estourava o gutter fixo da status line.
const ALLOWED_WIDE_SAFE = new Set(['ಠ']);

function offendingGlyphs(text) {
  return [...text].filter((ch) => {
    const code = ch.codePointAt(0);
    if (code >= 0x20 && code <= 0x7e) return false; // ASCII imprimível
    return !ALLOWED_WIDE_SAFE.has(ch);
  });
}

test('todo arquétipo tem rosto de exatamente 2 colunas', () => {
  assert.strictEqual(Object.keys(SLOT_KITS).length, ARCHETYPE_LIST.length);
  for (const archetype of ARCHETYPE_LIST) {
    const kit = SLOT_KITS[archetype];
    assert.ok(kit, `${archetype} sem kit de rosto`);
    assert.strictEqual([...kit.brow].length, 1, `${archetype}: sobrancelha de 1 coluna`);
    assert.strictEqual([...kit.eye].length, 1, `${archetype}: olho de 1 coluna`);
  }
});

test('rostos são distintos entre arquétipos', () => {
  const faces = ARCHETYPE_LIST.map((a) => SLOT_KITS[a].brow + SLOT_KITS[a].eye);
  assert.strictEqual(new Set(faces).size, faces.length, 'dois arquétipos com o mesmo rosto');
});

test('o arquétipo não muda a largura do desenho — regressão do slot variável', () => {
  // Bug original: {EYES} recebia `ಠ_ಠ` (3 chars) no Reclamão e `><` (2) nos
  // outros, então o corpo do pato desalinhava conforme o arquétipo.
  for (let stage = 0; stage < STAGES.length; stage++) {
    const shapes = ARCHETYPE_LIST.map((a) => fillBody(a, stage).map((l) => [...l].length));
    for (const widths of shapes) {
      assert.deepStrictEqual(widths, shapes[0], `estágio ${stage}: larguras divergem por arquétipo`);
    }
  }
});

test('nenhum glifo de largura ambígua no desenho', () => {
  for (let stage = 0; stage < STAGES.length; stage++) {
    for (const archetype of ARCHETYPE_LIST) {
      for (const line of fillBody(archetype, stage)) {
        assert.deepStrictEqual(
          offendingGlyphs(line),
          [],
          `estágio ${stage} / ${archetype}: glifo de largura não verificada em \`${line}\``
        );
      }
    }
  }
});

test('estágios crescem: cada um é mais alto ou mais largo que o anterior', () => {
  const size = (stage) => {
    const lines = fillBody(ARCHETYPES.SABIO, stage);
    return { rows: lines.length, cols: Math.max(...lines.map((l) => [...l].length)) };
  };
  for (let stage = 1; stage < STAGES.length; stage++) {
    const prev = size(stage - 1);
    const cur = size(stage);
    assert.ok(
      cur.rows > prev.rows || cur.cols > prev.cols,
      `estágio ${stage} não cresceu em relação ao ${stage - 1}`
    );
  }
});

test('fillBody satura fora da faixa de estágios', () => {
  assert.deepStrictEqual(fillBody(ARCHETYPES.SABIO, -1), fillBody(ARCHETYPES.SABIO, 0));
  assert.deepStrictEqual(fillBody(ARCHETYPES.SABIO, 99), fillBody(ARCHETYPES.SABIO, STAGES.length - 1));
});

test('arquétipo desconhecido cai no Sábio em vez de quebrar', () => {
  assert.deepStrictEqual(fillBody('Inexistente', 2), fillBody(ARCHETYPES.SABIO, 2));
});

test('card sem ANSI mostra nome, degrau, XP e o desenho do estágio certo', () => {
  const rank = RANKS[14]; // Lendário I
  const card = renderCard({
    name: 'Pato',
    archetype: ARCHETYPES.RECLAMAO,
    rankLabel: rank.label,
    rankHex: rank.hex,
    rankIndex: rank.index,
    xp: 24100,
    shiny: false,
    ansi: false,
  });
  assert.ok(card.includes('~ Pato ~'));
  assert.ok(card.includes('Lendário I'));
  assert.ok(card.includes('XP: 24100'));
  assert.ok(card.includes('\\|/'), 'Lendário desenha a coroa');
  assert.ok(!card.includes('\x1b'), 'ansi:false não pode emitir escape');
});

test('subir de raridade muda o desenho — a recompensa visível', () => {
  const drawFor = (rankIndex) =>
    renderCard({
      name: 'Pato',
      archetype: ARCHETYPES.RECLAMAO,
      rankLabel: RANKS[rankIndex].label,
      rankHex: RANKS[rankIndex].hex,
      rankIndex,
      xp: 0,
      shiny: false,
      ansi: false,
    });

  const perColor = RANKS.filter((r) => r.sub === 'III').map((r) => drawFor(r.index));
  assert.strictEqual(new Set(perColor).size, perColor.length, 'duas cores desenhando igual');

  // Dentro de uma cor, o sub-nível não muda a silhueta (só o rótulo).
  const raroIII = fillBody(ARCHETYPES.RECLAMAO, stageFromRank(6));
  const raroI = fillBody(ARCHETYPES.RECLAMAO, stageFromRank(8));
  assert.deepStrictEqual(raroIII, raroI);
});

test('statusline compacta cabe no gutter e traz a inicial da raridade', () => {
  for (const archetype of ARCHETYPE_LIST) {
    const text = renderStatusline({
      archetype,
      rankLabel: 'Lendário I',
      rankHex: '#FBBF24',
      shiny: false,
      ansi: false,
    });
    assert.deepStrictEqual(offendingGlyphs(text), []);
    assert.ok([...text].length <= SIDE_WIDTH, `${archetype}: \`${text}\` estourou o gutter`);
    assert.ok(text.endsWith(' L'));
  }
});
