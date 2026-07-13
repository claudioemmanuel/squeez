use crate::commands::compress_md::locale::Locale;

pub static PT_BR: Locale = Locale {
    code: "pt-BR",
    fillers: &[
        "só", "apenas", "realmente", "basicamente", "literalmente",
        "simplesmente", "certamente", "claramente", "obviamente",
        "exatamente", "praticamente", "tipo", "meio",
    ],
    // Includes definite/indefinite articles AND preposition+article contractions
    // (do/da/no/na/ao/à/pelo/pela etc.) — all should be dropped in pt-BR prose compression.
    articles: &[
        "o", "a", "os", "as",
        "um", "uma", "uns", "umas",
        "do", "da", "dos", "das",
        "no", "na", "nos", "nas",
        "ao", "aos", "à", "às",
        "pelo", "pela", "pelos", "pelas",
    ],
    phrases: &[
        "claro que sim",
        "com certeza",
        "fico feliz em ajudar",
        "ficarei feliz em",
        "deixa eu ",
        "deixe-me ",
        "vou te ajudar",
        "gostaria de ",
        "por favor, note que",
        "vale a pena ",
        "você pode considerar que",
        "você pode considerar",
        "de modo geral",
        "em geral",
        "como regra",
        "é importante notar que",
        "vale ressaltar que",
        "vale lembrar que",
    ],
    hedges: &[
        "talvez", "possivelmente", "provavelmente", "eventualmente",
        "porventura", "quiçá",
    ],
    conjunctions: &[
        " e", " ou", " mas", " porém", " contudo", " então", " logo", " pois",
    ],
    // E6: measured against o200k_base + cl100k_base across 3 real-sentence
    // contexts (bench/verify_tokens.py --substitutions, snapshot committed
    // at bench/substitutions_snapshot.json). Kept: ctx_wins >=2/3 (saves
    // >=1 token in at least 2 of 3 contexts). Pruned for scoring COSTS or
    // NEUTRAL (ctx_wins <2/3): sem->s/, com->c/, porque->pq, função->fn,
    // funções->fns, também->tb, você->vc, para->p/ — unlike the surviving
    // pairs below, these single/short-word contractions don't reliably
    // collapse to fewer BPE tokens.
    ultra_subs: &[
        ("por que",        "pq"),
        ("parâmetro",      "param"),
        ("parâmetros",     "params"),
        ("argumento",      "arg"),
        ("argumentos",     "args"),
        ("configuração",   "config"),
        ("configurações",  "configs"),
        ("documentação",   "docs"),
        ("diretório",      "dir"),
        ("repositório",    "repo"),
        ("aproximadamente","~"),
    ],
};
