'use strict';

const { ARCHETYPES, stageFromRank } = require('./engine');

// ---------------------------------------------------------------------------
// Rosto: sobrancelha + olho, exatamente 2 colunas.
//
// O desenho antigo tinha um slot {EYES} de largura variável (`ಠ_ಠ` = 3 chars no
// Reclamão, `><` = 2 nos outros), então o corpo do pato desalinhava conforme o
// arquétipo. Aqui todo kit ocupa 2 colunas fixas e a expressão vem do par
// sobrancelha+olho — é o que dá cara diferente a cada arquétipo sem precisar de
// nenhuma fala.
//
// REGRA DE GLIFO: só caracteres de largura de exibição 1. Os antigos `★`
// (U+2605), `⊙` (U+2299) e `▤` (U+25A4) são East Asian Ambiguous/Wide e ocupam
// 2 colunas em vários terminais — quebravam o alinhamento do card e, pior, o
// gutter fixo da status line. `ಠ` (U+0CA0) é Neutral, largura 1, e fica.
// ---------------------------------------------------------------------------

const SLOT_KITS = {
  [ARCHETYPES.RECLAMAO]: { brow: '-', eye: 'ಠ' }, // sobrancelha reta, olho cético
  [ARCHETYPES.MANDAO]: { brow: '\\', eye: 'o' }, // sobrancelha franzida
  [ARCHETYPES.FISCAL]: { brow: '=', eye: 'O' }, // olho arregalado, vigilante
  [ARCHETYPES.MOTIVADOR]: { brow: '^', eye: '*' }, // olho brilhando
  [ARCHETYPES.CAOTICO]: { brow: '~', eye: '@' }, // olho rodando
  [ARCHETYPES.SABIO]: { brow: '.', eye: '-' }, // olho semicerrado
};

function kitFor(archetype) {
  return SLOT_KITS[archetype] || SLOT_KITS[ARCHETYPES.SABIO];
}

/** Substitui o slot de rosto ({F} = sobrancelha+olho) numa linha de template. */
function fillFace(line, kit) {
  return line.replace('{F}', kit.brow + kit.eye);
}

// ---------------------------------------------------------------------------
// Cinco estágios de crescimento, um por cor de raridade (Comum … Lendário).
//
// A silhueta base é o pato de perfil que já funcionava na status line: bico
// `<`, cabeça `( )`, dorso `___`, barriga/asa `( ._> /`, quilha `` `---' ``.
// O card antigo usava uma variante quebrada dessa mesma silhueta.
//
// Subir de cor muda a forma inteira — é a recompensa visível que faltava: antes
// um Lendário I e um Comum III desenhavam exatamente o mesmo pato, só mudava a
// cor. O sub-nível (III/II/I) continua aparecendo apenas no rótulo.
// ---------------------------------------------------------------------------

const STAGES = [
  {
    name: 'filhote',
    lines: ['  __', '<({F})_', ' `~~\''],
  },
  {
    name: 'jovem',
    lines: ['   ___', ' <({F})__', '  ( ._> /', '   `---\''],
  },
  {
    name: 'adulto',
    lines: ['    ___', '  <({F})__', '   ( ._> /', ' ~~~`---\'~~~'],
  },
  {
    name: 'asa aberta',
    lines: ['    ___', '  <({F})__', '  \\( ._> /\\_', ' ~~~`----\'~~~'],
  },
  {
    name: 'coroado',
    lines: ['    \\|/', '    ___', '  <({F})__', '  \\( ._> /\\_', ' ~~~`----\'~~~'],
  },
];

/** Linhas do corpo para um estágio (0-4) e arquétipo, sem cor. */
function fillBody(archetype, stage = 1) {
  const kit = kitFor(archetype);
  const shape = STAGES[Math.max(0, Math.min(STAGES.length - 1, stage))];
  return shape.lines.map((line) => fillFace(line, kit));
}

// ---------------------------------------------------------------------------
// Silhueta da status line: 4 linhas de largura fixa, porque os medidores do HUD
// alinham nessa coluna. O estágio não pode mexer no tamanho aqui — só a linha
// do topo muda (penugem do filhote / dorso / coroa do lendário).
// ---------------------------------------------------------------------------

const SIDE_TEMPLATE = ['   {TOP}', ' <({F})__', '  ( ._> /', "   `---'"];
const SIDE_TOPS = ['_', '___', '___', '___', '\\|/'];
const SIDE_WIDTH = 11; // coluna fixa: os medidores da status line alinham nela

/** Linhas da arte de perfil, já com o rosto do arquétipo e largura fixa. */
function renderDuckArt(archetype, stage = 1) {
  const kit = kitFor(archetype);
  const top = SIDE_TOPS[Math.max(0, Math.min(SIDE_TOPS.length - 1, stage))];
  return SIDE_TEMPLATE.map((line) =>
    fillFace(line.replace('{TOP}', top), kit).padEnd(SIDE_WIDTH)
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
 * `rankHex` vem do motor de ranking (lib/engine.js RANKS[i].hex) e `rankIndex`
 * escolhe o estágio do desenho (lib/engine.js stageFromRank).
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
      if (i < 2 || i >= bodyLines.length + 2) return line; // título/rodapé sem cor
      if (shiny) {
        const color = SHINY_PALETTE[i % 2];
        return ansiFg(hexToRgb(color)) + line + ANSI_RESET;
      }
      return ansiFg(hexToRgb(rankHex)) + line + ANSI_RESET;
    })
    .join('\n');
}

/**
 * Versão compacta de 1 linha, pra status line (espaço é caro ali).
 * Mesma cabeça de perfil do card, achatada: bico `<`, rosto, dorso `__`.
 */
function renderStatusline({ archetype, rankLabel, rankHex, shiny, ansi = true }) {
  const kit = kitFor(archetype);
  const compact = `<(${kit.brow}${kit.eye})__`;
  const star = rankLabel.split(' ')[0][0]; // primeira letra da cor, ex: "L" de Lendário
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
