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
