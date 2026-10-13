'use strict';

const { ARCHETYPES, stageFromRank } = require('./engine');

// ---------------------------------------------------------------------------
// Face: eyebrow + eye, exactly 2 columns.
//
// The old art had a variable-width {EYES} slot (`ಠ_ಠ` = 3 chars on the
// Grumbler, `><` = 2 on the others), so the duck's body misaligned per
// archetype. Here every kit occupies 2 fixed columns and the expression comes
// from the eyebrow+eye pair — that is what gives each archetype a different
// face without needing any speech.
//
// GLYPH RULE: display-width-1 characters only. The old `★` (U+2605), `⊙`
// (U+2299) and `▤` (U+25A4) are East Asian Ambiguous/Wide and take 2 columns in
// many terminals — they broke the card alignment and, worse, the fixed gutter
// of the status line. `ಠ` (U+0CA0) is Neutral, width 1, and stays.
// ---------------------------------------------------------------------------

const SLOT_KITS = {
  [ARCHETYPES.RECLAMAO]: { brow: '-', eye: 'ಠ' }, // straight brow, skeptical eye
  [ARCHETYPES.MANDAO]: { brow: '\\', eye: 'o' }, // furrowed brow
  [ARCHETYPES.FISCAL]: { brow: '=', eye: 'O' }, // wide-open, watchful eye
  [ARCHETYPES.MOTIVADOR]: { brow: '^', eye: '*' }, // sparkling eye
  [ARCHETYPES.CAOTICO]: { brow: '~', eye: '@' }, // spinning eye
  [ARCHETYPES.SABIO]: { brow: '.', eye: '-' }, // half-closed eye
};

function kitFor(archetype) {
  return SLOT_KITS[archetype] || SLOT_KITS[ARCHETYPES.SABIO];
}

/** Replaces the face slot ({F} = eyebrow+eye) in a template line. */
function fillFace(line, kit) {
  return line.replace('{F}', kit.brow + kit.eye);
}

// ---------------------------------------------------------------------------
// Five growth stages, one per rarity color (Common … Legendary).
//
// The base silhouette is the side-view duck that already worked on the status
// line: beak `<`, head `( )`, back `___`, belly/wing `( ._> /`, keel `` `---' ``.
// The old card used a broken variant of that same silhouette.
//
// Going up a color changes the whole shape — it is the visible reward that was
// missing: before, a Legendary I and a Common III drew exactly the same duck,
// only the color changed. The sub-level (III/II/I) still shows only in the label.
// ---------------------------------------------------------------------------

const STAGES = [
  {
    name: 'hatchling',
    lines: ['  __', '<({F})_', ' `~~\''],
  },
  {
    name: 'young',
    lines: ['   ___', ' <({F})__', '  ( ._> /', '   `---\''],
  },
  {
    name: 'adult',
    lines: ['    ___', '  <({F})__', '   ( ._> /', ' ~~~`---\'~~~'],
  },
  {
    name: 'wings spread',
    lines: ['    ___', '  <({F})__', '  \\( ._> /\\_', ' ~~~`----\'~~~'],
  },
  {
    name: 'crowned',
    lines: ['    \\|/', '    ___', '  <({F})__', '  \\( ._> /\\_', ' ~~~`----\'~~~'],
  },
];

/** Body lines for a stage (0-4) and archetype, without color. */
function fillBody(archetype, stage = 1) {
  const kit = kitFor(archetype);
  const shape = STAGES[Math.max(0, Math.min(STAGES.length - 1, stage))];
  return shape.lines.map((line) => fillFace(line, kit));
}

// ---------------------------------------------------------------------------
// Status line silhouette: 4 fixed-width lines, because the HUD meters align on
// that column. The stage can't change the size here — only the top line changes
// (hatchling fluff / back / legendary crown).
// ---------------------------------------------------------------------------

const SIDE_TEMPLATE = ['   {TOP}', ' <({F})__', '  ( ._> /', "   `---'"];
const SIDE_TOPS = ['_', '___', '___', '___', '\\|/'];
const SIDE_WIDTH = 11; // fixed column: the status line meters align on it

/** Side-art lines, already with the archetype face and fixed width. */
function renderDuckArt(archetype, stage = 1) {
  const kit = kitFor(archetype);
  const top = SIDE_TOPS[Math.max(0, Math.min(SIDE_TOPS.length - 1, stage))];
  return SIDE_TEMPLATE.map((line) =>
    fillFace(line.replace('{TOP}', top), kit).padEnd(SIDE_WIDTH)
  );
}

// ---------------------------------------------------------------------------
// ANSI true-color (24-bit) per rarity. Shiny = palette alternating per line
// instead of a solid color.
// ---------------------------------------------------------------------------

function hexToRgb(hex) {
  const n = parseInt(hex.replace('#', ''), 16);
  return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255 };
}

function ansiFg({ r, g, b }) {
  return `\x1b[38;2;${r};${g};${b}m`;
}

const ANSI_RESET = '\x1b[0m';
const SHINY_PALETTE = ['#22D3EE', '#F472B6']; // alternating cyan / pink

/**
 * Renders the full (multi-line) card with name, rank and color.
 * `rankHex` comes from the ranking engine (lib/engine.js RANKS[i].hex) and
 * `rankIndex` picks the art stage (lib/engine.js stageFromRank).
 */
function renderCard({ name, archetype, rankLabel, rankHex, rankIndex = 0, xp, shiny, ansi = true }) {
  const stage = stageFromRank(rankIndex);
  const bodyLines = fillBody(archetype, stage);
  const lines = [
    `~ ${name} ~`,
    '',
    ...bodyLines,
    '',
    `${archetype} · ${rankLabel}${shiny ? ' · shiny ✦' : ''}`,
    `XP: ${xp}`,
  ];

  if (!ansi) return lines.join('\n');

  return lines
    .map((line, i) => {
      if (i < 2 || i >= bodyLines.length + 2) return line; // title/footer without color
      if (shiny) {
        const color = SHINY_PALETTE[i % 2];
        return ansiFg(hexToRgb(color)) + line + ANSI_RESET;
      }
      return ansiFg(hexToRgb(rankHex)) + line + ANSI_RESET;
    })
    .join('\n');
}

/**
 * Compact 1-line version, for the status line (space is expensive there).
 * Same side-view head as the card, flattened: beak `<`, face, back `__`.
 */
function renderStatusline({ archetype, rankLabel, rankHex, shiny, ansi = true }) {
  const kit = kitFor(archetype);
  const compact = `<(${kit.brow}${kit.eye})__`;
  const star = rankLabel.split(' ')[0][0]; // first letter of the color, e.g. "L" for Legendary
  const text = `${compact} ${star}`;

  if (!ansi) return text;
  const color = shiny ? SHINY_PALETTE[0] : rankHex;
  return ansiFg(hexToRgb(color)) + text + ANSI_RESET;
}

module.exports = {
  SLOT_KITS,
  STAGES,
  SIDE_WIDTH,
  fillBody,
  renderDuckArt,
  hexToRgb,
  ansiFg,
  ANSI_RESET,
  renderCard,
  renderStatusline,
};
