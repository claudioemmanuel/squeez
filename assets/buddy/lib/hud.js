'use strict';

// Right column of the status line: project/branch/model, 5h window, 7d window
// and context pressure. All derived from the payload the host sends on stdin
// + the usage API (lib/usage.js). No npm dependency.

const fs = require('fs');
const path = require('path');

const { ansiFg, hexToRgb, ANSI_RESET } = require('./art');
const { configuredContextWindow } = require('./squeez');

const DIM = '\x1b[2m';
const BAR_CELLS = 9;
const HEAT = [
  { max: 49, hex: '#4ADE80' }, // green
  { max: 79, hex: '#FBBF24' }, // amber
  { max: 100, hex: '#F87171' }, // red
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

/** "2h 4m" / "3d 5h" / "now" from a reset ISO. */
function untilReset(iso) {
  if (!iso) return '';
  const target = new Date(iso).getTime();
  if (!Number.isFinite(target)) return '';
  const left = target - Date.now();
  if (left <= 0) return 'now';
  const minutes = Math.floor(left / 60000);
  const days = Math.floor(minutes / 1440);
  const hours = Math.floor((minutes % 1440) / 60);
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes % 60}m`;
  return `${minutes}m`;
}

/**
 * Context tokens from the assistant's last turn in the transcript JSONL.
 * Reads only the tail of the file — transcripts run into tens of MB.
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
        continue; // first line of the window usually comes truncated
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
        /* fd already closed */
      }
    }
  }
}

const LONG_CONTEXT_RE = /\[1m\]|-1m\b|context-1m|\b1m\s+context\b/i;
// Fable / Mythos have 1M natively — id and display_name come raw
// (`claude-fable-5-1`, "Fable 5.1"), with no marker at all.
const NATIVE_1M_RE = /fable|mythos/i;

/**
 * The host's context window. Precedence:
 *   1. `context_window_tokens` from config.ini — the user pinned it, it is
 *      authoritative;
 *   2. `context_window.context_window_size` from the status line payload — the
 *      host reports the real size. Opus 5.5 with 1M arrives as
 *      "claude-opus-5-5" / "Opus 5.5", with no marker at all, and the sniff
 *      below would fall back to 200k;
 *   3. a 1M marker in the `id` OR in the `display_name` (host without the
 *      field);
 *   4. 200k.
 *
 * The `id` alone is not enough (#199): in a 1M session Claude Code writes
 * `claude-opus-5` raw, without `[1m]` — verified in 70/70 assistant records of
 * a real 1M session. `display_name` is what carries the signal, "Opus 5 (1M
 * context)". Accepts a string (id only) for compatibility with callers that
 * already passed it that way.
 */
function contextWindow(model, pinnedTokens, hostWindowSize) {
  const pinned = Number(pinnedTokens);
  if (Number.isFinite(pinned) && pinned > 0) return pinned;
  const reported = Number(hostWindowSize);
  if (Number.isFinite(reported) && reported > 0) return reported;
  const probe =
    typeof model === 'string'
      ? model
      : `${(model && model.id) || ''} ${(model && model.display_name) || ''}`;
  if (NATIVE_1M_RE.test(probe)) return 1_000_000;
  return LONG_CONTEXT_RE.test(probe) ? 1_000_000 : 200_000;
}

/** Current branch without spawning git: reads .git/HEAD walking up the tree. */
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
  if (percent == null) return `${label.padEnd(3)}${dim('no data', ansi)}`;
  const hex = heatHex(percent);
  const head = `${label.padEnd(3)}${String(percent).padStart(3)}% `;
  const tail = suffix ? ` ${dim(suffix, ansi)}` : '';
  return paint(head + bar(percent), hex, ansi) + tail;
}

const CURRENCY_SYMBOL = { USD: '$', BRL: 'R$', EUR: '€', GBP: '£' };

/** "$42.37" / "$200" — cents only when they exist. */
function money(value, currency) {
  if (value == null) return '';
  return `${CURRENCY_SYMBOL[currency] || ''}${Number.isInteger(value) ? value : value.toFixed(2)}`;
}

/**
 * The HUD's two middle lines.
 *
 * On an enterprise account every rate limit window comes back null (#198), and
 * two "no data" lines are indistinguishable from a broken buddy. There the
 * limit that bites is the credit one, so the spend meter takes its place.
 * With no windows AND no spend the honest placeholder stays — better to say
 * "no data" than to make things up.
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
 * Builds the 4 lines of the right column.
 * `input` is the host JSON; `usage` comes from lib/usage.js (may be null).
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
  const window = contextWindow(
    input.model,
    configuredContextWindow(),
    input.context_window && input.context_window.context_window_size
  );
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
