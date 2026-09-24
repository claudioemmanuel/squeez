'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { renderDuckArt, SLOT_KITS } = require('../lib/art');
const {
  bar,
  heatHex,
  untilReset,
  contextWindow,
  compactTokens,
  buildHudLines,
} = require('../lib/hud');
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
  // Fable / Mythos: 1M nativo, sem marcador no id nem no display_name.
  assert.strictEqual(contextWindow('claude-fable-5-1'), 1_000_000);
  assert.strictEqual(contextWindow({ id: 'claude-fable-5-1', display_name: 'Fable 5.1' }), 1_000_000);
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

test('janela de contexto: context_window_size do host vence o sniff pelo nome', () => {
  // Opus 5.5 com 1M: o payload da status line traz id e display_name crus
  // ("claude-opus-5-5", "Opus 5.5") e o tamanho real em context_window.
  const opus55 = { id: 'claude-opus-5-5', display_name: 'Opus 5.5' };
  assert.strictEqual(contextWindow(opus55, undefined, 1_000_000), 1_000_000);
  // O host também diz quando a janela é 200k, mesmo com marcador no nome.
  assert.strictEqual(contextWindow({ id: 'claude-opus-5[1m]' }, undefined, 200_000), 200_000);
  // Pinado pelo usuário continua autoritativo.
  assert.strictEqual(contextWindow(opus55, 200_000, 1_000_000), 200_000);
  // Host sem o campo (versão antiga) ou com lixo cai no sniff de antes.
  assert.strictEqual(contextWindow(opus55, undefined, undefined), 200_000);
  assert.strictEqual(contextWindow(opus55, undefined, 0), 200_000);
  assert.strictEqual(contextWindow({ id: 'claude-fable-5-1' }, undefined, 'x'), 1_000_000);
});

test('buildHudLines mede o contexto contra a janela que o host informou', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'squeez-hud-'));
  const transcript = path.join(dir, 't.jsonl');
  const usage = { input_tokens: 2, cache_creation_input_tokens: 0, cache_read_input_tokens: 299_998, output_tokens: 10 };
  fs.writeFileSync(transcript, `${JSON.stringify({ type: 'assistant', message: { usage } })}\n`);
  const lines = buildHudLines({
    input: {
      cwd: dir,
      transcript_path: transcript,
      model: { id: 'claude-opus-5-5', display_name: 'Opus 5.5' },
      context_window: { context_window_size: 1_000_000 },
    },
    usage: null,
    state: { xp: 0 },
    rank: { label: 'Comum', hex: '#888888' },
    ansi: false,
  });
  const tail = lines[3];
  assert.match(tail, /300k\/1\.0M/);
  assert.doesNotMatch(tail, /\/200k/);
  fs.rmSync(dir, { recursive: true, force: true });
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

const HUD_ARGS = { state: { xp: 10 }, rank: { label: 'Comum I', hex: '#4ADE80' }, ansi: false };
const hudRows = (usage) =>
  buildHudLines({ input: { model: { id: 'claude-sonnet-5' } }, usage, ...HUD_ARGS });

test('medidores de plano consumidor seguem sendo 5h e 7d', () => {
  const rows = hudRows({ fiveHour: 19, sevenDay: 45, spend: null, extraUsage: null });
  assert.match(rows[1], /^5h\s+19%/);
  assert.match(rows[2], /^7d\s+45%/);
});

test('plano enterprise cai para medidor de gasto quando toda janela é null', () => {
  // #198: com as janelas null o HUD mostrava duas linhas "sem dados", que o
  // usuário não distingue de "buddy quebrado". spend é o limite que importa lá.
  const rows = hudRows({
    fiveHour: null,
    sevenDay: null,
    spend: 21,
    spendUsed: 42.37,
    spendLimit: 200,
    spendCurrency: 'USD',
    extraUsage: 8,
  });
  assert.match(rows[1], /^\$\s+21%/, 'linha 2 vira medidor de gasto');
  assert.match(rows[1], /\$42\.37\/\$200/, 'sufixo mostra usado/limite');
  assert.match(rows[2], /^\+\s+8%/, 'linha 3 vira extra usage');
  assert.ok(
    !`${rows[1]}\n${rows[2]}`.includes('sem dados'),
    'nenhuma das duas linhas de uso fica como placeholder morto'
  );
});

test('sem janelas e sem spend, o placeholder honesto permanece', () => {
  const rows = hudRows({ fiveHour: null, sevenDay: null, spend: null, extraUsage: null });
  assert.match(rows[1], /^5h\s+sem dados/);
  assert.match(rows[2], /^7d\s+sem dados/);
});

test('spend sem extra_usage mantém 7d como placeholder', () => {
  const rows = hudRows({ fiveHour: null, sevenDay: null, spend: 55, extraUsage: null });
  assert.match(rows[1], /^\$\s+55%/);
  assert.match(rows[2], /^7d\s+sem dados/);
});

test('tokens compactos', () => {
  assert.strictEqual(compactTokens(950), '950');
  assert.strictEqual(compactTokens(127_400), '127k');
  assert.strictEqual(compactTokens(1_000_000), '1.0M');
});
