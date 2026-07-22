// ADHD output-shaping axis, orthogonal to `persona`.
//
// `persona` picks *how compressed* the model's prose is (off → ultra);
// `focus` picks *how it is structured*. They stack: persona=ultra keeps
// dropping articles while focus=adhd forces next-action-first ordering,
// numbered steps and a per-turn state line.
//
// The text is injected by `init.rs` into the session banner and by each
// host adapter into its `<!-- squeez:start -->` memory block, right after
// the persona text. Rules adapted from ayghri/i-have-adhd (MIT).

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum Focus {
    /// Opt-in: the default banner and prose stay exactly as they were.
    #[default]
    Off,
    Adhd,
}

pub fn from_str(s: &str) -> Focus {
    match s.trim().to_lowercase().as_str() {
        "adhd" | "on" | "true" | "1" => Focus::Adhd,
        _ => Focus::Off,
    }
}

pub fn as_str(f: Focus) -> &'static str {
    match f {
        Focus::Off => "off",
        Focus::Adhd => "adhd",
    }
}

/// Suffix for the "Persona: … " status line — empty unless focus is on, so
/// the default banner never grows by a token.
pub fn banner_suffix(f: Focus) -> &'static str {
    match f {
        Focus::Off => "",
        Focus::Adhd => " | Focus: adhd",
    }
}

/// Rule 9 — cap an advisory burst at 5 lines. The queue that fed this was
/// already drained, so the tail is dropped rather than deferred; the count
/// line keeps that honest instead of silently hiding it.
pub fn cap_advisories(warnings: Vec<String>, f: Focus) -> Vec<String> {
    const MAX: usize = 5;
    if f == Focus::Off || warnings.len() <= MAX {
        return warnings;
    }
    let dropped = warnings.len() - MAX;
    let mut out: Vec<String> = warnings.into_iter().take(MAX).collect();
    out.push(format!("[squeez: +{} more advisories suppressed]", dropped));
    out
}

const ADHD: &str = include_str!("../../assets/focus_adhd.md");
const ADHD_PT_BR: &str = include_str!("../../assets/focus_adhd_pt_br.md");

pub fn text(f: Focus) -> &'static str {
    match f {
        Focus::Off => "",
        Focus::Adhd => ADHD,
    }
}

pub fn text_with_lang(f: Focus, lang: &str) -> &'static str {
    match (f, lang) {
        (Focus::Off, _) => "",
        (Focus::Adhd, "pt-BR") => ADHD_PT_BR,
        (Focus::Adhd, _) => ADHD,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_round_trip() {
        for f in [Focus::Off, Focus::Adhd] {
            assert_eq!(from_str(as_str(f)), f);
        }
    }

    #[test]
    fn from_str_case_insensitive() {
        assert_eq!(from_str("ADHD"), Focus::Adhd);
        assert_eq!(from_str(" Adhd "), Focus::Adhd);
    }

    #[test]
    fn from_str_unknown_is_off() {
        assert_eq!(from_str("focused"), Focus::Off);
        assert_eq!(from_str(""), Focus::Off);
    }

    #[test]
    fn default_is_off() {
        assert_eq!(Focus::default(), Focus::Off);
    }

    #[test]
    fn off_text_is_empty() {
        assert!(text(Focus::Off).is_empty());
        assert!(text_with_lang(Focus::Off, "pt-BR").is_empty());
    }

    #[test]
    fn adhd_text_is_lang_selected() {
        assert!(text_with_lang(Focus::Adhd, "en").contains("Lead with next action"));
        assert!(text_with_lang(Focus::Adhd, "pt-BR").contains("próxima ação"));
    }
}
