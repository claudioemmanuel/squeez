#!/usr/bin/env node
'use strict';

// Full squeez status line: side-view duck on the left, meters on the right
// (project/model, 5h window, 7d window, context pressure). Replaces the
// third-party HUD — nothing is chained.

const fs = require('fs');

const { readState, currentRank } = require('../lib/state');
const { renderDuckArt, renderStatusline, ansiFg, hexToRgb, ANSI_RESET } = require('../lib/art');
const { stageFromRank } = require('../lib/engine');
const { buildHudLines } = require('../lib/hud');
const { getUsage } = require('../lib/usage');
const { currentLang } = require('../lib/i18n');

const SHINY_CYAN = '#22D3EE';
const GUTTER = 11; // fixed width of the duck column (SIDE_TEMPLATE)

function readStdin() {
  try {
    return JSON.parse(fs.readFileSync(0, 'utf8'));
  } catch {
    return {};
  }
}

function render(input, usage, ansi = !process.env.NO_COLOR) {
  const state = readState();
  const lang = currentLang();
  const rank = currentRank(state, lang);

  const duckHex = state.shiny ? SHINY_CYAN : rank.hex;
  const art = renderDuckArt(state.archetype, stageFromRank(rank.index)).map((line) =>
    ansi ? ansiFg(hexToRgb(duckHex)) + line + ANSI_RESET : line
  );
  const hud = buildHudLines({ input, usage, state, rank, ansi, lang });

  const lines = art.map((duckLine, i) => `${duckLine} ${hud[i] || ''}`.trimEnd());

  if (state.lastComment) {
    const raw = state.lastComment;
    const comment = raw.length > 64 ? `${raw.slice(0, 61)}…` : raw;
    const quoted = `"${comment}"`;
    lines.push(`${' '.repeat(GUTTER)} ${ansi ? `\x1b[2m${quoted}${ANSI_RESET}` : quoted}`);
  }

  return lines.join('\n');
}

function main() {
  // --compact: a single line (`<(ಠ)__ C`), for anyone who wants to chain it into another HUD.
  if (process.argv.includes('--compact')) {
    const state = readState();
    const rank = currentRank(state);
    process.stdout.write(
      `${renderStatusline({
        archetype: state.archetype,
        rankLabel: rank.label,
        rankHex: rank.hex,
        shiny: state.shiny,
        ansi: true,
      })}\n`
    );
    return;
  }

  const input = readStdin();
  getUsage((usage) => {
    process.stdout.write(`${render(input, usage)}\n`);
  });
}

if (require.main === module) main();

module.exports = { render };
