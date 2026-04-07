use squeez::commands::persona::{as_str, from_str, text, Persona};

#[test]
fn default_is_ultra() {
    assert_eq!(Persona::default(), Persona::Ultra);
}

#[test]
fn from_str_round_trip_all_levels() {
    for p in [Persona::Off, Persona::Lite, Persona::Full, Persona::Ultra] {
        assert_eq!(from_str(as_str(p)), p);
    }
}

#[test]
fn from_str_case_insensitive() {
    assert_eq!(from_str("ULTRA"), Persona::Ultra);
    assert_eq!(from_str("Lite"), Persona::Lite);
    assert_eq!(from_str("OFF"), Persona::Off);
}

#[test]
fn from_str_unknown_falls_back_to_default() {
    assert_eq!(from_str("nonsense"), Persona::default());
    assert_eq!(from_str(""), Persona::Off);
}

#[test]
fn off_text_is_empty() {
    assert_eq!(text(Persona::Off), "");
}

#[test]
fn ultra_text_contains_ultra_marker() {
    let t = text(Persona::Ultra);
    assert!(t.contains("ultra"));
    assert!(t.len() > 100);
}

#[test]
fn full_text_contains_caveman() {
    let t = text(Persona::Full);
    assert!(t.to_lowercase().contains("caveman"));
}

#[test]
fn lite_text_is_shorter_than_full() {
    assert!(text(Persona::Lite).len() < text(Persona::Full).len());
}
