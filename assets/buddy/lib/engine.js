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
 * Progressão geométrica: base 60, razão ~1.45. Cada degrau custa ~45% mais que
 * o anterior; o teto é 24.100 XP acumulados.
 *
 * A curva anterior (base 150, razão 1.55, teto 125.730) foi calibrada sobre uma
 * premissa errada — "sessão pesada = ~70 XP". A medição real do primeiro pato
 * em produção deu ~700 XP/dia, 10x isso, porque o gatilho de "bug resolvido"
 * pagava 10 XP toda vez que a palavra "error" aparecia numa saída e o comando
 * seguinte vinha limpo. Corrigido o vazamento (ver hooks/post-tool-use.js), a
 * torneira honesta é a economia de tokens do squeez.
 *
 * PREMISSA DE CALIBRAÇÃO: ~20 XP por dia ativo
 *   = até 8 XP de ação (teto duro por sessão) + ~12 XP de economia
 *     (~300k tokens líquidos/dia a 25.000:1, ver lib/squeez.js).
 *
 * As duas colunas abaixo medem coisas diferentes — a confusão entre elas foi o
 * que quebrou a calibração anterior:
 *
 *   degrau        XP acumulado    tempo acumulado a 20 XP/dia ativo
 *   Comum II              60      ~3 dias        <- fisgada inicial rápida
 *   Incomum I          1.100      ~2 meses
 *   Raro I             3.650      ~6 meses
 *   Épico I           11.400      ~1,5 ano
 *   Lendário I        24.100      ~3,3 anos      <- endgame, não um checkpoint
 *
 * Mexer aqui reprecifica todo o histórico: o XP acumulado não muda, só o degrau
 * que ele compra. Para recalibrar o ritmo prefira ajustar TOKENS_PER_XP em
 * lib/squeez.js — a taxa é o botão de calibração, a curva é a forma.
 */
const RANK_THRESHOLDS = [
  0, 60, 150, 275, 460, 720, 1100, 1660, 2470, 3650, 5350, 7800, 11400, 16600,
  24100,
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

/**
 * Estágio de crescimento do desenho (0-4), um por cor de raridade. É o que dá
 * recompensa visível ao subir de rank: o sub-nível (III/II/I) só muda o rótulo,
 * a cor muda a silhueta inteira. Ver lib/art.js STAGES.
 */
function stageFromRank(index) {
  return Math.floor(Math.max(0, Math.min(MAX_RANK_INDEX, index)) / SUB_LEVELS.length);
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
  stageFromRank,
  applyDecay,
  daysBetween,
};
