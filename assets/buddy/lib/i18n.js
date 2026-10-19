'use strict';

// Display strings for the buddy, per language. The language follows squeez's
// own `lang` key in config.ini (`lang = en` default, `lang = pt-BR` the other
// supported value). State files persist language-neutral ids (lib/engine.js
// ARCHETYPES, intensity ids, rank color ids); this module maps them to text at
// render time, and maps legacy labels from either language back to the ids.

const { configuredLang } = require('./squeez');

const LANGS = ['en', 'pt-BR'];

/** Same mapping as the Rust `Locale::from_code`: pt variants → pt-BR, rest → en. */
function normalizeLang(raw) {
  return ['pt', 'pt-BR', 'pt_BR', 'pt-br'].includes(raw) ? 'pt-BR' : 'en';
}

/** Language configured in squeez's config.ini, normalized. */
function currentLang() {
  return normalizeLang(configuredLang());
}

const ARCHETYPE_LABELS = {
  en: {
    bossy: 'Bossy',
    grumbler: 'Grumbler',
    chaotic: 'Chaotic',
    inspector: 'Inspector/Guardian',
    sage: 'Sage',
    motivator: 'Motivator',
  },
  'pt-BR': {
    bossy: 'Mandão',
    grumbler: 'Reclamão',
    chaotic: 'Caótico',
    inspector: 'Fiscal/Guardião',
    sage: 'Sábio',
    motivator: 'Motivador',
  },
};

const INTENSITY_LABELS = {
  en: { mild: 'mild', moderate: 'moderate', intense: 'intense' },
  'pt-BR': { mild: 'leve', moderate: 'moderado', intense: 'intenso' },
};

const RANK_COLOR_LABELS = {
  en: { common: 'Common', uncommon: 'Uncommon', rare: 'Rare', epic: 'Epic', legendary: 'Legendary' },
  'pt-BR': { common: 'Comum', uncommon: 'Incomum', rare: 'Raro', epic: 'Épico', legendary: 'Lendário' },
};

/** Id for `value` when it is an id or a label in any language; else `value` as-is. */
function idFrom(tables, value) {
  if (Object.prototype.hasOwnProperty.call(tables.en, value)) return value;
  for (const lang of LANGS) {
    const hit = Object.keys(tables[lang]).find((id) => tables[lang][id] === value);
    if (hit) return hit;
  }
  return value;
}

function labelFrom(tables, value, lang) {
  const id = idFrom(tables, value);
  return (tables[normalizeLang(lang)] || tables.en)[id] || tables.en[id] || value;
}

const archetypeId = (value) => idFrom(ARCHETYPE_LABELS, value);
const intensityId = (value) => idFrom(INTENSITY_LABELS, value);
const archetypeLabel = (value, lang = currentLang()) => labelFrom(ARCHETYPE_LABELS, value, lang);
const intensityLabel = (value, lang = currentLang()) => labelFrom(INTENSITY_LABELS, value, lang);

/** "Épico I" / "Epic I" for a lib/engine.js RANKS entry. */
function rankLabel(rank, lang = currentLang()) {
  return `${labelFrom(RANK_COLOR_LABELS, rank.color, lang)} ${rank.sub}`;
}

const MESSAGES = {
  en: {
    numberLocale: 'en-US',
    noData: 'no data',
    now: 'now',
    intro: ({ name, archetype, intensity, rank }) =>
      `You have a companion named ${name} (archetype: ${archetype}, intensity: ${intensity}, rank: ${rank}).`,
    talkRule:
      'When the user calls the companion by name directly in the conversation, let it reply in first person, in the tone of its archetype, and you (Claude) stay out of that speech.',
    quietRule:
      'Otherwise, do not mention the companion unless it has something relevant to comment about what is happening in the session.',
    currentCard: (card) => `Current card:\n${card}`,
    pace: ({ perDay, xp, days, since }) =>
      `Pace: ${perDay} XP/day (${xp} XP in ${days} ${days === 1 ? 'day' : 'days'} since ${since})`,
    paceSplit: ({ granted, fromAction }) => `  squeez savings: ${granted} XP · action: ${fromAction} XP`,
    topRank: 'Next rank: none — Legendary I is the top.',
    neverAtPace: 'never at this pace',
    nextRank: ({ label, missing, eta }) => `Next rank: ${label} in ${missing} XP (${eta})`,
    savingsTotal: (lifetime) => `Accumulated squeez savings: ${lifetime} net tokens`,
    savingsNext: ({ left, rate }) => `Next savings XP in: ${left} tokens (rate ${rate}:1)`,
    created: (date) => `Created: ${date}`,
    pet: (name) => `${name} wags its tail, happy. (no XP effect)`,
    renameUsage: 'usage: buddy-cli.js rename <name>',
    renamed: (name) => `From now on it answers to ${name}.`,
    reset: ({ had, name }) =>
      `XP zeroed (was ${had}). ${name} is still the same duck — only the score went back to the start.`,
  },
  'pt-BR': {
    numberLocale: 'pt-BR',
    noData: 'sem dados',
    now: 'agora',
    intro: ({ name, archetype, intensity, rank }) =>
      `Você tem um companheiro chamado ${name} (arquétipo: ${archetype}, intensidade: ${intensity}, degrau: ${rank}).`,
    talkRule:
      'Quando o usuário chamar o companheiro pelo nome diretamente na conversa, deixe-o responder em primeira pessoa, no tom do arquétipo, e você (Claude) fica de fora dessa fala.',
    quietRule:
      'Fora isso, não mencione o companheiro a menos que ele tenha algo relevante a comentar sobre o que está acontecendo na sessão.',
    currentCard: (card) => `Card atual:\n${card}`,
    pace: ({ perDay, xp, days, since }) =>
      `Ritmo: ${perDay} XP/dia (${xp} XP em ${days} ${days === 1 ? 'dia' : 'dias'} desde ${since})`,
    paceSplit: ({ granted, fromAction }) => `  economia squeez: ${granted} XP · ação: ${fromAction} XP`,
    topRank: 'Próximo degrau: nenhum — Lendário I é o topo.',
    neverAtPace: 'nunca nesse ritmo',
    nextRank: ({ label, missing, eta }) => `Próximo degrau: ${label} em ${missing} XP (${eta})`,
    savingsTotal: (lifetime) => `Economia squeez acumulada: ${lifetime} tokens líquidos`,
    savingsNext: ({ left, rate }) => `Próximo XP de economia em: ${left} tokens (taxa ${rate}:1)`,
    created: (date) => `Criado em: ${date}`,
    pet: (name) => `${name} balança o rabinho, feliz. (sem efeito em XP)`,
    renameUsage: 'uso: buddy-cli.js rename <nome>',
    renamed: (name) => `Agora ele atende por ${name}.`,
    reset: ({ had, name }) =>
      `XP zerado (era ${had}). ${name} continua o mesmo pato — só o placar voltou pro começo.`,
  },
};

/** Message table for `lang` (defaults to the configured language). */
function messages(lang = currentLang()) {
  return MESSAGES[normalizeLang(lang)];
}

module.exports = {
  normalizeLang,
  currentLang,
  archetypeId,
  intensityId,
  archetypeLabel,
  intensityLabel,
  rankLabel,
  messages,
};
