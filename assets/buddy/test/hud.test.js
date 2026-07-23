'use strict';

const test = require('node:test');
const assert = require('node:assert');

const { renderDuckArt, SLOT_KITS } = require('../lib/art');
const { bar, heatHex, untilReset, contextWindow, compactTokens } = require('../lib/hud');

test('arte de perfil tem largura fixa em todos os arquétipos', () => {
  for (const archetype of Object.keys(SLOT_KITS)) {
    const lines = renderDuckArt(archetype);
    assert.strictEqual(lines.length, 4, `${archetype}: 4 linhas`);
    for (const line of lines) {
      assert.strictEqual([...line].length, 11, `${archetype}: coluna de 11 (\`${line}\`)`);
    }
  }
});

test('olho do arquétipo aparece na cabeça', () => {
  assert.ok(renderDuckArt('Reclamão')[1].includes('ಠ'));
  assert.ok(renderDuckArt('Motivador')[1].includes('★'));
});

test('barra tem sempre 9 células e respeita o percentual', () => {
  assert.strictEqual([...bar(0)].length, 9);
  assert.strictEqual([...bar(100)].length, 9);
  assert.strictEqual(bar(0), '░'.repeat(9));
  assert.strictEqual(bar(100), '█'.repeat(9));
  assert.ok(bar(50).startsWith('█'));
});

test('cor da barra escala com a pressão', () => {
  assert.strictEqual(heatHex(10), '#4ADE80');
  assert.strictEqual(heatHex(65), '#FBBF24');
  assert.strictEqual(heatHex(95), '#F87171');
});

test('reset formata dias/horas/minutos e nunca fica negativo', () => {
  const inMinutes = (m) => new Date(Date.now() + m * 60000).toISOString();
  assert.match(untilReset(inMinutes(45)), /^4[45]m$/); // floor: o relógio anda durante o teste
  assert.match(untilReset(inMinutes(125)), /^2h \d+m$/);
  assert.match(untilReset(inMinutes(60 * 30)), /^1d \d+h$/);
  assert.strictEqual(untilReset(inMinutes(-5)), 'agora');
  assert.strictEqual(untilReset(null), '');
  assert.strictEqual(untilReset('não é data'), '');
});

test('janela de contexto: sufixo [1m] vale 1M, resto 200k', () => {
  assert.strictEqual(contextWindow('claude-opus-4-8[1m]'), 1_000_000);
  assert.strictEqual(contextWindow('claude-sonnet-5'), 200_000);
  assert.strictEqual(contextWindow(undefined), 200_000);
});

test('tokens compactos', () => {
  assert.strictEqual(compactTokens(950), '950');
  assert.strictEqual(compactTokens(127_400), '127k');
  assert.strictEqual(compactTokens(1_000_000), '1.0M');
});
