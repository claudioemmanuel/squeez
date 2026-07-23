#!/usr/bin/env node
'use strict';

// Status line completa do squeez: pato de perfil à esquerda, medidores à
// direita (projeto/modelo, janela de 5h, janela de 7d, pressão de contexto).
// Substitui o HUD de terceiro — nada é encadeado.

const fs = require('fs');

const { readState, currentRank } = require('../lib/state');
const { renderDuckArt, renderStatusline, ansiFg, hexToRgb, ANSI_RESET } = require('../lib/art');
const { buildHudLines } = require('../lib/hud');
const { getUsage } = require('../lib/usage');

const SHINY_CYAN = '#22D3EE';
const GUTTER = 11; // largura fixa da coluna do pato (SIDE_TEMPLATE)

function readStdin() {
  try {
    return JSON.parse(fs.readFileSync(0, 'utf8'));
  } catch {
    return {};
  }
}

function render(input, usage, ansi = !process.env.NO_COLOR) {
  const state = readState();
  const rank = currentRank(state);

  const duckHex = state.shiny ? SHINY_CYAN : rank.hex;
  const art = renderDuckArt(state.archetype).map((line) =>
    ansi ? ansiFg(hexToRgb(duckHex)) + line + ANSI_RESET : line
  );
  const hud = buildHudLines({ input, usage, state, rank, ansi });

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
  // --compact: uma linha só (`<(ಠ)__ C`), pra quem quiser encadear em outro HUD.
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
