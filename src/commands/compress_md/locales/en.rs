use crate::commands::compress_md::locale::Locale;

pub static EN: Locale = Locale {
    code: "en",
    fillers:      &["just","really","basically","actually","simply","sure","certainly"],
    articles:     &["the","a","an"],
    phrases:      &[
        "of course",
        "i'd be happy to",
        "let me ",
        "i'll help you",
        "i would like to",
        "please note that",
        "it might be worth",
        "you could consider",
        "in general",
        "as a rule",
    ],
    hedges:       &["perhaps","maybe"],
    conjunctions: &[" and"," or"," but"," so"],
    // E6: measured against o200k_base + cl100k_base across 3 real-sentence
    // contexts (bench/verify_tokens.py --substitutions, snapshot committed
    // at bench/substitutions_snapshot.json). Every EN letter/word
    // substitution that was here — with→w/, without→w/o, because→b/c,
    // function→fn, parameter→param, argument(s)→arg(s), configuration→
    // config, documentation→docs, directory→dir, repository→repo,
    // between→btw, versus→vs, approximately→~ — scored ctx_wins 0/3 or 1/3
    // (COSTS or NEUTRAL on average). BPE tokenizes common English words as
    // single tokens already; the abbreviation is usually the SAME token
    // count, sometimes worse. Pruned to empty rather than kept as
    // net-zero-or-negative "compression". Article/filler/phrase dropping
    // above is a different mechanism (whole-word deletion, not
    // substitution) and is unaffected — that one does save tokens.
    ultra_subs:   &[],
};
