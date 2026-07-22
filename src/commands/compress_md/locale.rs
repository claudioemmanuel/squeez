use crate::commands::compress_md::locales;

#[derive(Copy, Clone, Debug)]
pub struct Locale {
    #[allow(dead_code)]
    pub code:         &'static str,
    pub fillers:      &'static [&'static str],
    pub articles:     &'static [&'static str],
    pub phrases:      &'static [&'static str],
    pub hedges:       &'static [&'static str],
    pub conjunctions: &'static [&'static str],
    pub ultra_subs:   &'static [(&'static str, &'static str)],
}

impl Locale {
    pub fn from_code(code: &str) -> &'static Locale {
        match code {
            "pt" | "pt-BR" | "pt_BR" | "pt-br" => &locales::PT_BR,
            _ => &locales::EN,
        }
    }
}

/// Words that are frequent in one language and absent from the other. Words
/// that exist in both — `a`, `o`, `as`, `no`, `do`, `na` — are deliberately
/// excluded: they are exactly the ones that make a wrong locale destructive.
const PT_MARKERS: &[&str] = &[
    "que", "não", "para", "uma", "isso", "você", "quando", "também", "então",
    "mas", "seja", "como", "mais", "está", "são", "sem", "cada", "tudo",
];
const EN_MARKERS: &[&str] = &[
    "the", "and", "you", "with", "this", "that", "are", "not", "when", "from",
    "your", "only", "into", "does", "each", "than", "them", "what",
];

/// Sniffs which language a document is written in.
///
/// `lang` in the config says which language squeez *writes its own prose in* —
/// it says nothing about the language of a memory file squeez is about to
/// rewrite. Trusting it there is destructive: the pt-BR list drops `no`, `as`,
/// `do` and `na` (articles and preposition+article contractions in Portuguese),
/// which in English are a negation, a conjunction and a verb. Compressing an
/// English `CLAUDE.md` under `lang = pt-BR` turned "No features beyond what was
/// asked" into " features beyond what was asked" — the instruction inverted.
///
/// So the locale follows the *content*, and only an explicit `--lang` overrides
/// it. Ties and marker-free documents fall back to EN, whose list holds no word
/// that is ambiguous in Portuguese.
pub fn detect(text: &str) -> &'static Locale {
    let lower = text.to_lowercase();
    let (mut pt, mut en) = (0usize, 0usize);
    for word in lower.split(|c: char| !c.is_alphabetic()) {
        if word.is_empty() {
            continue;
        }
        if PT_MARKERS.contains(&word) {
            pt += 1;
        } else if EN_MARKERS.contains(&word) {
            en += 1;
        }
    }
    if pt > en {
        &locales::PT_BR
    } else {
        &locales::EN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_english_prose() {
        let s = "No features beyond what was asked. Do not refactor things that are not broken.";
        assert_eq!(detect(s).code, "en");
    }

    #[test]
    fn detect_portuguese_prose() {
        let s = "Não crie recurso além do que foi pedido. Isso é para manter o código simples.";
        assert_eq!(detect(s).code, "pt-BR");
    }

    #[test]
    fn detect_falls_back_to_en_when_marker_free() {
        assert_eq!(detect("").code, "en");
        assert_eq!(detect("# Title\n\n```sh\nls -la\n```\n").code, "en");
    }

    #[test]
    fn detect_follows_the_dominant_half_of_a_mixed_file() {
        // A pt-BR squeez block injected into an English CLAUDE.md: the English
        // body must still win, or the body is the half that gets mangled.
        let s = "Não use isso. \n\
                 The rules that follow are the ones that matter, and you should \
                 read them when you are not sure what your next change does.";
        assert_eq!(detect(s).code, "en");
    }
}
