'use strict';

/**
 * Motor determinístico do pato-buddy.
 *
 * Tudo neste arquivo é função pura (sem I/O, sem Date.now() direto nos cálculos
 * centrais) de propósito: o gatilho, o arquétipo e a intensidade da personalidade
 * precisam ser auditáveis e testáveis por tabela de casos, não "emergentes".
 * Só o texto final do comentário (fora deste arquivo) é gerado por LLM.
 */

const STAT_NAMES = ['DEBUGGING', 'PATIENCE', 'CHAOS', 'WISDOM', 'SNARK'];

// ---------------------------------------------------------------------------
// Hash + PRNG determinísticos (mesma família de algoritmo documentada para o
// /buddy original: hash de identidade -> seed -> PRNG determinístico).
// ---------------------------------------------------------------------------

/** FNV-1a de 32 bits. Determinístico, sem dependências externas. */
function fnv1a(str) {
  let hash = 0x811c9dc5;
  for (let i = 0; i < str.length; i++) {
    hash ^= str.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

/** PRNG mulberry32: rápido, determinístico, qualidade suficiente pra isso. */
function mulberry32(seed) {
  let a = seed >>> 0;
  return function next() {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const SALT = 'pato-buddy-2026';

function seedFromIdentity(identity) {
  return fnv1a(SALT + ':' + identity);
}

// ---------------------------------------------------------------------------
// Geração de stats (peak/dump)
// ---------------------------------------------------------------------------

/**
 * Gera os 5 stats (0-100) a partir de uma identidade estável (ex: account id).
 * Sempre há exatamente 1 stat de pico (o maior) e 1 de vale (o menor) --
 * a árvore de decisão do arquétipo depende dessa garantia.
 */
function generateStats(identity) {
  const rand = mulberry32(seedFromIdentity(identity));

  const order = [...STAT_NAMES];
  // Fisher-Yates determinístico usando o mesmo PRNG
  for (let i = order.length - 1; i > 0; i--) {
    const j = Math.floor(rand() * (i + 1));
    [order[i], order[j]] = [order[j], order[i]];
  }

  const peakName = order[0];
  const dumpName = order[1];
  const midNames = order.slice(2);

  const stats = {};
  stats[peakName] = 65 + Math.floor(rand() * 36); // 65-100
  stats[dumpName] = Math.floor(rand() * 31); // 0-30
  for (const name of midNames) {
    stats[name] = 31 + Math.floor(rand() * 34); // 31-64
  }

  return { stats, peakName, dumpName };
}

// ---------------------------------------------------------------------------
// Árvore de decisão: stats -> arquétipo
// ---------------------------------------------------------------------------

const ARCHETYPES = {
  MANDAO: 'Mandão',
  RECLAMAO: 'Reclamão',
  CAOTICO: 'Caótico',
  FISCAL: 'Fiscal/Guardião',
  SABIO: 'Sábio',
  MOTIVADOR: 'Motivador',
};

/**
 * Regra de prioridade fechada (ver spec). Cobre as 20 combinações possíveis
 * de (peak, dump) entre os 5 stats. Testado exaustivamente em engine.test.js.
 */
function resolveArchetype(peakName, dumpName) {
  if (dumpName === 'PATIENCE' && (peakName === 'DEBUGGING' || peakName === 'CHAOS')) {
    return ARCHETYPES.MANDAO;
  }
  if (peakName === 'SNARK') return ARCHETYPES.RECLAMAO;
  if (peakName === 'CHAOS') return ARCHETYPES.CAOTICO;
  if (peakName === 'DEBUGGING') return ARCHETYPES.FISCAL;
  if (peakName === 'WISDOM' && dumpName !== 'SNARK') return ARCHETYPES.SABIO;
  if (peakName === 'PATIENCE') return ARCHETYPES.MOTIVADOR;
  return ARCHETYPES.SABIO; // fallback: peak WISDOM, dump SNARK ("Sábio ácido")
}

/** Intensidade = distância entre pico e vale, não valor absoluto do pico. */
function resolveIntensity(stats, peakName, dumpName) {
  const gap = stats[peakName] - stats[dumpName];
  if (gap <= 25) return 'leve';
  if (gap <= 50) return 'moderado';
  return 'intenso';
}

/** Monta a identidade completa e determinística de um buddy. */
function buildBuddy(identity) {
  const { stats, peakName, dumpName } = generateStats(identity);
  const archetype = resolveArchetype(peakName, dumpName);
  const intensity = resolveIntensity(stats, peakName, dumpName);
  return { identity, stats, peakName, dumpName, archetype, intensity };
}

// ---------------------------------------------------------------------------
// Ranking (XP -> degrau), 5 cores x 3 sub-níveis = 15 degraus
// ---------------------------------------------------------------------------

const RANK_COLORS = ['Comum', 'Incomum', 'Raro', 'Épico', 'Lendário'];
const SUB_LEVELS = ['III', 'II', 'I']; // III é o mais baixo dentro da cor

const RANK_HEX = {
  Comum: '#9CA3AF',
  Incomum: '#34D399',
  Raro: '#60A5FA',
  Épico: '#C084FC',
  Lendário: '#FBBF24',
};

/**
 * Curva de XP acumulado necessário para alcançar cada um dos 15 degraus.
 *
 * Progressão geométrica (base 150, razão ~1.55), no formato dos MMOs: cada
 * degrau custa ~55% mais que o anterior. A curva antiga era quase linear
 * (total 6.500) e dava para varrer os degraus em poucas sessões — Épico saía
 * numa tarde. Aqui o total é 125.730 XP, ~19x maior, e o custo por degrau
 * cresce de 150 XP até 44.710 XP.
 *
 * Efeito prático (sessão pesada ~70 XP: ~40 edits + 3 commits + 2 suítes
 * verdes + 1 bug resolvido):
 *
 *   Comum II        150 XP        ~2 sessões    <- fisgada inicial rápida
 *   Incomum I     2.170 XP       ~12 sessões
 *   Raro I        8.810 XP       ~46 sessões
 *   Épico I      33.560 XP      ~171 sessões
 *   Lendário I  125.730 XP      ~639 sessões    <- endgame, não um checkpoint
 *
 * No topo uma ação isolada vale ~0,002% do degrau e uma sessão inteira ~0,08%,
 * que é a escala de grind de MMO pretendida. Mexer aqui reprecifica todo o
 * histórico: o XP acumulado não muda, só o degrau que ele compra.
 */
const RANK_THRESHOLDS = [
  0, 150, 380, 740, 1300, 2170, 3510, 5590, 8810, 13810, 21560, 33560, 52170,
  81020, 125730,
];

const RANKS = RANK_COLORS.flatMap((color) =>
  SUB_LEVELS.map((sub) => ({ color, sub, label: `${color} ${sub}` }))
).map((r, i) => ({ ...r, index: i, xpRequired: RANK_THRESHOLDS[i], hex: RANK_HEX[r.color] }));

const MAX_RANK_INDEX = RANKS.length - 1;
const FLOOR_RANK_INDEX = 0; // Comum III -- piso de decaimento

function rankIndexFromXp(xp) {
  let idx = 0;
  for (let i = 0; i < RANKS.length; i++) {
    if (xp >= RANK_THRESHOLDS[i]) idx = i;
    else break;
  }
  return idx;
}

function rankFromIndex(index) {
  const clamped = Math.max(0, Math.min(MAX_RANK_INDEX, index));
  return RANKS[clamped];
}

// ---------------------------------------------------------------------------
// Decaimento por inatividade
// ---------------------------------------------------------------------------

const GRACE_PERIOD_DAYS = 3;
const DECAY_INTERVAL_DAYS = 7;

/**
 * Calcula o degrau efetivo aplicando decaimento por inatividade.
 * - até GRACE_PERIOD_DAYS sem sessão: nenhum efeito
 * - a cada DECAY_INTERVAL_DAYS além do grace: -1 sub-degrau
 * - nunca cai abaixo de FLOOR_RANK_INDEX (Comum III)
 * - retorna o índice efetivo; o XP acumulado NUNCA é alterado por isso --
 *   decaimento afeta só a exibição/degrau atual, não o histórico de XP.
 */
function applyDecay(xp, daysSinceLastSession) {
  const baseIndex = rankIndexFromXp(xp);
  const idleDays = Math.max(0, daysSinceLastSession - GRACE_PERIOD_DAYS);
  if (idleDays <= 0) return baseIndex;
  const steps = Math.floor(idleDays / DECAY_INTERVAL_DAYS);
  return Math.max(FLOOR_RANK_INDEX, baseIndex - steps);
}

function daysBetween(isoA, isoB) {
  const msPerDay = 1000 * 60 * 60 * 24;
  const a = new Date(isoA).getTime();
  const b = new Date(isoB).getTime();
  return Math.max(0, Math.floor((b - a) / msPerDay));
}

module.exports = {
  STAT_NAMES,
  ARCHETYPES,
  RANKS,
  RANK_COLORS,
  SUB_LEVELS,
  MAX_RANK_INDEX,
  FLOOR_RANK_INDEX,
  GRACE_PERIOD_DAYS,
  DECAY_INTERVAL_DAYS,
  fnv1a,
  mulberry32,
  seedFromIdentity,
  generateStats,
  resolveArchetype,
  resolveIntensity,
  buildBuddy,
  rankIndexFromXp,
  rankFromIndex,
  applyDecay,
  daysBetween,
};
