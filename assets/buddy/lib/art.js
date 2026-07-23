'use strict';

const { ARCHETYPES } = require('./engine');

// ---------------------------------------------------------------------------
// Corpo-base do pato (silhueta fixa, única espécie). {EYES}/{MOUTH}/{DECOR}
// são preenchidos pelo kit de slot do arquétipo.
// ---------------------------------------------------------------------------

const BODY_TEMPLATE = ['   __      {DECOR}', ' <({EYES})___', '  ({MOUTH}> /', '   `---\'  '];

// Kit de slot por arquétipo: olhos, boca, decoração. `side` é o olho de perfil,
// usado na miniatura de 1 linha da status line. Arte original, não
// reaproveitada de nenhum projeto de terceiros.
const SLOT_KITS = {
  [ARCHETYPES.RECLAMAO]: { eyes: 'ಠ_ಠ', mouth: '~', decor: '', side: 'ಠ' },
  [ARCHETYPES.MANDAO]: { eyes: '><', mouth: '=', decor: '>>', side: '><' },
  [ARCHETYPES.FISCAL]: { eyes: '⊙⊙', mouth: '‾', decor: '▤', side: '⊙' },
  [ARCHETYPES.MOTIVADOR]: { eyes: '★★', mouth: '◡', decor: '✧', side: '★' },
  [ARCHETYPES.CAOTICO]: { eyes: '@◠', mouth: '~', decor: '?!', side: '@' },
  [ARCHETYPES.SABIO]: { eyes: '° °', mouth: '-', decor: '…', side: '°' },
};

// Silhueta de perfil, 4 linhas — usada na status line, onde o pato divide a
// linha com os medidores. Só o olho ({SIDE}) muda por arquétipo.
const SIDE_TEMPLATE = ['    __', ' <({SIDE} )___', '  ( ._> /', "   `---'"];
const SIDE_WIDTH = 11; // coluna fixa: os medidores da status line alinham nela

/** Linhas da arte de perfil, já com o olho do arquétipo e largura fixa. */
function renderDuckArt(archetype) {
  const kit = SLOT_KITS[archetype] || SLOT_KITS[ARCHETYPES.SABIO];
  const eye = (kit.side || 'o').slice(0, 1); // olho de perfil é 1 caractere
  return SIDE_TEMPLATE.map((line) => line.replace('{SIDE}', eye).padEnd(SIDE_WIDTH));
}

function fillBody(archetype) {
  const kit = SLOT_KITS[archetype] || SLOT_KITS[ARCHETYPES.SABIO];
  return BODY_TEMPLATE.map((line) =>
    line.replace('{EYES}', kit.eyes).replace('{MOUTH}', kit.mouth).replace('{DECOR}', kit.decor)
  );
}

// ---------------------------------------------------------------------------
// Cor ANSI true-color (24-bit) por raridade. Shiny = paleta alternada por
// linha em vez de cor sólida.
// ---------------------------------------------------------------------------

function hexToRgb(hex) {
  const n = parseInt(hex.replace('#', ''), 16);
  return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255 };
}

function ansiFg({ r, g, b }) {
  return `\x1b[38;2;${r};${g};${b}m`;
}

const ANSI_RESET = '\x1b[0m';
const SHINY_PALETTE = ['#22D3EE', '#F472B6']; // ciano / rosa alternados

/**
 * Renderiza o card completo (multi-linha) com nome, degrau e cor.
 * `rankHex` vem do motor de ranking (lib/engine.js RANKS[i].hex).
 */
function renderCard({ name, archetype, rankLabel, rankHex, xp, shiny, ansi = true }) {
  const bodyLines = fillBody(archetype);
  const lines = [`~ ${name} ~`, '', ...bodyLines, '', `${archetype} · ${rankLabel}${shiny ? ' · shiny ✦' : ''}`, `XP: ${xp}`];

  if (!ansi) return lines.join('\n');

  return lines
    .map((line, i) => {
      if (i < 2 || i >= bodyLines.length + 2) return line; // título/rodapé sem cor
      const isBody = true;
      if (shiny && isBody) {
        const color = SHINY_PALETTE[i % 2];
        return ansiFg(hexToRgb(color)) + line + ANSI_RESET;
      }
      return ansiFg(hexToRgb(rankHex)) + line + ANSI_RESET;
    })
    .join('\n');
}

/**
 * Versão compacta de 1 linha, pra status line (espaço é caro ali).
 * Mesma silhueta de perfil do card, achatada: bico `<`, cabeça, corpo `__`.
 */
function renderStatusline({ archetype, rankLabel, rankHex, shiny, ansi = true }) {
  const kit = SLOT_KITS[archetype] || SLOT_KITS[ARCHETYPES.SABIO];
  const compact = `<(${kit.side || kit.eyes})__`;
  const star = rankLabel.split(' ')[0][0]; // primeira letra da cor, ex: "L" de Lendário
  const text = `${compact} ${star}`;

  if (!ansi) return text;
  const color = shiny ? SHINY_PALETTE[0] : rankHex;
  return ansiFg(hexToRgb(color)) + text + ANSI_RESET;
}

module.exports = {
  SLOT_KITS,
  fillBody,
  renderDuckArt,
  hexToRgb,
  ansiFg,
  ANSI_RESET,
  renderCard,
  renderStatusline,
};
