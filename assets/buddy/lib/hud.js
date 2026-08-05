'use strict';

// Coluna direita da status line: projeto/branch/modelo, janela de 5h, janela de
// 7d e pressão de contexto. Tudo derivado do payload que o host manda no stdin
// + a API de uso (lib/usage.js). Sem dependência npm.

const fs = require('fs');
const path = require('path');

const { ansiFg, hexToRgb, ANSI_RESET } = require('./art');
const { configuredContextWindow } = require('./squeez');

const DIM = '\x1b[2m';
const BAR_CELLS = 9;
const HEAT = [
  { max: 49, hex: '#4ADE80' }, // verde
  { max: 79, hex: '#FBBF24' }, // âmbar
  { max: 100, hex: '#F87171' }, // vermelho
];

function heatHex(percent) {
  return (HEAT.find((step) => percent <= step.max) || HEAT[HEAT.length - 1]).hex;
}

function paint(text, hex, ansi) {
  return ansi ? ansiFg(hexToRgb(hex)) + text + ANSI_RESET : text;
}

function dim(text, ansi) {
  return ansi ? DIM + text + ANSI_RESET : text;
}

function bar(percent) {
  const filled = Math.round((percent / 100) * BAR_CELLS);
  return '█'.repeat(filled) + '░'.repeat(BAR_CELLS - filled);
}

/** "2h 4m" / "3d 5h" / "agora" a partir de um ISO de reset. */
function untilReset(iso) {
  if (!iso) return '';
  const target = new Date(iso).getTime();
  if (!Number.isFinite(target)) return '';
  const left = target - Date.now();
  if (left <= 0) return 'agora';
  const minutes = Math.floor(left / 60000);
  const days = Math.floor(minutes / 1440);
  const hours = Math.floor((minutes % 1440) / 60);
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes % 60}m`;
  return `${minutes}m`;
}

/**
 * Tokens de contexto do último turno do assistente no transcript JSONL.
 * Lê só a cauda do arquivo — transcripts passam de dezenas de MB.
 */
function contextTokens(transcriptPath) {
  if (!transcriptPath) return null;
  let fd;
  try {
    const size = fs.statSync(transcriptPath).size;
    const window = Math.min(size, 256 * 1024);
    const buf = Buffer.alloc(window);
    fd = fs.openSync(transcriptPath, 'r');
    fs.readSync(fd, buf, 0, window, size - window);
    const lines = buf.toString('utf8').split('\n');
    for (let i = lines.length - 1; i >= 0; i--) {
      const line = lines[i].trim();
      if (!line.startsWith('{')) continue;
      let entry;
      try {
        entry = JSON.parse(line);
      } catch {
        continue; // primeira linha da janela costuma vir cortada
      }
      const usage = entry && entry.message && entry.message.usage;
      if (!usage) continue;
      const total =
        (usage.input_tokens || 0) +
        (usage.cache_read_input_tokens || 0) +
        (usage.cache_creation_input_tokens || 0);
      if (total > 0) return total;
    }
    return null;
  } catch {
    return null;
  } finally {
    if (fd !== undefined) {
      try {
        fs.closeSync(fd);
      } catch {
        /* fd já fechado */
      }
    }
  }
}

const LONG_CONTEXT_RE = /\[1m\]|-1m\b|context-1m|\b1m\s+context\b/i;

/**
 * Janela de contexto do host. Precedência:
 *   1. `context_window_tokens` do config.ini — o usuário pinou, é autoritativo;
 *   2. marcador de 1M no `id` OU no `display_name`;
 *   3. 200k.
 *
 * O `id` sozinho não serve (#199): numa sessão 1M o Claude Code grava
 * `claude-opus-5` cru, sem `[1m]` — verificado em 70/70 registros assistant de
 * uma sessão 1M real. Quem carrega o sinal é `display_name`, "Opus 5 (1M context)".
 * Aceita string (só o id) por compatibilidade com quem já chamava assim.
 */
function contextWindow(model, pinnedTokens) {
  const pinned = Number(pinnedTokens);
  if (Number.isFinite(pinned) && pinned > 0) return pinned;
  const probe =
    typeof model === 'string'
      ? model
      : `${(model && model.id) || ''} ${(model && model.display_name) || ''}`;
  return LONG_CONTEXT_RE.test(probe) ? 1_000_000 : 200_000;
}

/** Branch atual sem spawnar git: lê .git/HEAD subindo a árvore. */
function gitBranch(cwd) {
  let dir = cwd;
  for (let i = 0; i < 12 && dir; i++) {
    try {
      const head = fs.readFileSync(path.join(dir, '.git', 'HEAD'), 'utf8').trim();
      const match = head.match(/^ref: refs\/heads\/(.+)$/);
      return match ? match[1] : head.slice(0, 7);
    } catch {
      const parent = path.dirname(dir);
      if (parent === dir) break;
      dir = parent;
    }
  }
  return null;
}

function compactTokens(n) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1000) return `${Math.round(n / 1000)}k`;
  return String(n);
}

function meter(label, percent, suffix, ansi) {
  if (percent == null) return `${label.padEnd(3)}${dim('sem dados', ansi)}`;
  const hex = heatHex(percent);
  const head = `${label.padEnd(3)}${String(percent).padStart(3)}% `;
  const tail = suffix ? ` ${dim(suffix, ansi)}` : '';
  return paint(head + bar(percent), hex, ansi) + tail;
}

const CURRENCY_SYMBOL = { USD: '$', BRL: 'R$', EUR: '€', GBP: '£' };

/** "$42.37" / "$200" — centavos só quando existem. */
function money(value, currency) {
  if (value == null) return '';
  return `${CURRENCY_SYMBOL[currency] || ''}${Number.isInteger(value) ? value : value.toFixed(2)}`;
}

/**
 * As duas linhas do meio do HUD.
 *
 * Em conta enterprise toda janela de rate limit vem null (#198), e duas linhas
 * "sem dados" são indistinguíveis de buddy quebrado. Lá o limite que morde é o
 * de crédito, então o medidor de gasto assume o lugar. Sem janelas E sem gasto
 * o placeholder honesto fica — melhor dizer "sem dados" do que inventar.
 */
function usageRows(usage, ansi) {
  const hasWindows = usage && (usage.fiveHour != null || usage.sevenDay != null);
  if (!hasWindows && usage && usage.spend != null) {
    const suffix = [money(usage.spendUsed, usage.spendCurrency), money(usage.spendLimit, usage.spendCurrency)]
      .filter(Boolean)
      .join('/');
    return [
      meter('$', usage.spend, suffix, ansi),
      usage.extraUsage != null
        ? meter('+', usage.extraUsage, 'extra', ansi)
        : meter('7d', null, '', ansi),
    ];
  }
  return [
    meter('5h', usage ? usage.fiveHour : null, usage ? untilReset(usage.fiveHourResetsAt) : '', ansi),
    meter('7d', usage ? usage.sevenDay : null, usage ? untilReset(usage.sevenDayResetsAt) : '', ansi),
  ];
}

/**
 * Monta as 4 linhas da coluna direita.
 * `input` é o JSON do host; `usage` vem de lib/usage.js (pode ser null).
 */
function buildHudLines({ input, usage, state, rank, ansi = true }) {
  const cwd = (input.workspace && input.workspace.current_dir) || input.cwd || process.cwd();
  const project = path.basename(cwd);
  const branch = gitBranch(cwd);
  const model = (input.model && input.model.display_name) || '';

  const head = [
    project,
    branch ? dim(`(${branch})`, ansi) : '',
    model ? dim('·', ansi) + ` ${model}` : '',
  ]
    .filter(Boolean)
    .join(' ');

  const used = contextTokens(input.transcript_path);
  const window = contextWindow(input.model, configuredContextWindow());
  const ctxPct = used == null ? null : Math.min(100, Math.round((used / window) * 100));
  const ctxSuffix = used == null ? '' : `${compactTokens(used)}/${compactTokens(window)}`;

  const xp = `XP ${state.xp}`;
  const tail = [
    meter('ctx', ctxPct, ctxSuffix, ansi),
    dim('·', ansi),
    paint(`${rank.label}`, rank.hex, ansi),
    dim(`· ${xp}`, ansi),
  ].join(' ');

  const [first, second] = usageRows(usage, ansi);

  return [head, first, second, tail];
}

module.exports = {
  buildHudLines,
  contextTokens,
  contextWindow,
  gitBranch,
  untilReset,
  bar,
  heatHex,
  compactTokens,
};
