'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { renderDuckArt, SLOT_KITS } = require('../lib/art');
const { bar, heatHex, untilReset, contextWindow, compactTokens } = require('../lib/hud');
const { configuredContextWindow } = require('../lib/squeez');

test('arte de perfil tem largura fixa em todos os arquétipos', () => {
  for (const archetype of Object.keys(SLOT_KITS)) {
    const lines = renderDuckArt(archetype);
    assert.strictEqual(lines.length, 4, `${archetype}: 4 linhas`);
    for (const line of lines) {
      assert.strictEqual([...line].length, 11, `${archetype}: coluna de 11 (\`${line}\`)`);
    }
  }
});

test('rosto do arquétipo aparece na cabeça', () => {
  assert.ok(renderDuckArt('Reclamão')[1].includes('-ಠ'));
  assert.ok(renderDuckArt('Motivador')[1].includes('^*'));
});

test('estágio muda o topo da silhueta sem mexer na largura', () => {
  assert.ok(renderDuckArt('Sábio', 0)[0].includes('_'));
  assert.ok(renderDuckArt('Sábio', 4)[0].includes('\\|/'), 'lendário ganha coroa');
  for (let stage = 0; stage <= 4; stage++) {
    for (const line of renderDuckArt('Sábio', stage)) {
      assert.strictEqual([...line].length, 11, `estágio ${stage}: coluna de 11`);
    }
  }
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

test('janela de contexto: display_name decide quando o id não carrega marcador', () => {
  // Regressão #199. Numa sessão 1M o Claude Code grava o id cru — verificado em
  // 70/70 registros assistant de uma sessão 1M real. Só display_name distingue.
  assert.strictEqual(
    contextWindow({ id: 'claude-opus-5', display_name: 'Opus 5 (1M context)' }),
    1_000_000
  );
  assert.strictEqual(contextWindow({ id: 'claude-opus-5', display_name: 'Opus 5' }), 200_000);
  assert.strictEqual(contextWindow({ id: 'claude-opus-5[1m]', display_name: 'Opus 5' }), 1_000_000);
  assert.strictEqual(contextWindow({}), 200_000);
});

test('janela de contexto: context_window_tokens pinado vence qualquer sniff', () => {
  assert.strictEqual(contextWindow({ id: 'claude-opus-5' }, 1_000_000), 1_000_000);
  assert.strictEqual(
    contextWindow({ id: 'x', display_name: 'Opus 5 (1M context)' }, 200_000),
    200_000
  );
  // Valor inválido/ausente não pode sequestrar a precedência.
  assert.strictEqual(contextWindow({ id: 'claude-opus-5' }, 0), 200_000);
  assert.strictEqual(contextWindow({ id: 'claude-opus-5' }, undefined), 200_000);
});

test('configuredContextWindow lê context_window_tokens do config.ini', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'squeez-cfg-'));
  const prev = process.env.SQUEEZ_DIR;
  process.env.SQUEEZ_DIR = dir;
  try {
    assert.strictEqual(configuredContextWindow(), 0, 'sem config.ini → 0');

    fs.writeFileSync(
      path.join(dir, 'config.ini'),
      '# comentário\ncontext_window_tokens = 1000000\nmax_lines = 40\n'
    );
    assert.strictEqual(configuredContextWindow(), 1_000_000);

    fs.writeFileSync(path.join(dir, 'config.ini'), '# context_window_tokens = 999\n');
    assert.strictEqual(configuredContextWindow(), 0, 'linha comentada não conta');
  } finally {
    if (prev === undefined) delete process.env.SQUEEZ_DIR;
    else process.env.SQUEEZ_DIR = prev;
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('tokens compactos', () => {
  assert.strictEqual(compactTokens(950), '950');
  assert.strictEqual(compactTokens(127_400), '127k');
  assert.strictEqual(compactTokens(1_000_000), '1.0M');
});
