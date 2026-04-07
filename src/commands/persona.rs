// Caveman persona for prompt-side output compression.
// The text is injected by `init.rs` into the session banner (Claude Code)
// and into the <!-- squeez:start --> block in copilot-instructions.md
// (Copilot CLI). It nudges the agent to produce terser output —
// squeez can't intercept the model's response stream, only suggest.

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Persona {
    Off,
    Lite,
    Full,
    Ultra,
}

impl Default for Persona {
    fn default() -> Self {
        // User chose Ultra as the default during PR2 design.
        Persona::Ultra
    }
}

pub fn from_str(s: &str) -> Persona {
    match s.trim().to_lowercase().as_str() {
        "off" | "false" | "0" | "" => Persona::Off,
        "lite" => Persona::Lite,
        "full" => Persona::Full,
        "ultra" => Persona::Ultra,
        _ => Persona::default(),
    }
}

pub fn as_str(p: Persona) -> &'static str {
    match p {
        Persona::Off => "off",
        Persona::Lite => "lite",
        Persona::Full => "full",
        Persona::Ultra => "ultra",
    }
}

const LITE: &str = include_str!("../../assets/persona_lite.md");
const FULL: &str = include_str!("../../assets/persona_full.md");
const ULTRA: &str = include_str!("../../assets/persona_ultra.md");

pub fn text(p: Persona) -> &'static str {
    match p {
        Persona::Off => "",
        Persona::Lite => LITE,
        Persona::Full => FULL,
        Persona::Ultra => ULTRA,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_round_trip() {
        for p in [Persona::Off, Persona::Lite, Persona::Full, Persona::Ultra] {
            assert_eq!(from_str(as_str(p)), p);
        }
    }

    #[test]
    fn from_str_case_insensitive() {
        assert_eq!(from_str("ULTRA"), Persona::Ultra);
        assert_eq!(from_str("Lite"), Persona::Lite);
    }

    #[test]
    fn from_str_unknown_falls_back_to_default() {
        assert_eq!(from_str("nonsense"), Persona::default());
    }

    #[test]
    fn off_text_is_empty() {
        assert!(text(Persona::Off).is_empty());
    }

    #[test]
    fn ultra_text_contains_ultra_marker() {
        let t = text(Persona::Ultra);
        assert!(t.contains("ultra"));
        assert!(t.len() > 50);
    }

    #[test]
    fn default_is_ultra() {
        assert_eq!(Persona::default(), Persona::Ultra);
    }
}
