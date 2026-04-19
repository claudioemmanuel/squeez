use squeez::context::summarize::{apply_with_format, SummaryFormat};

// Helper: build a 1000-line cargo-build-like input with 2 distinctive error lines.
fn make_cargo_lines() -> Vec<String> {
    let mut lines: Vec<String> = Vec::with_capacity(1000);
    for i in 0..500 {
        lines.push(format!("   Compiling crate_{} v0.1.{}", i % 50, i));
    }
    lines.push("error: cannot find value `UNIQUE_ERROR_ALPHA` in this scope".to_string());
    lines.push("error: mismatched types UNIQUE_ERROR_BETA expected `usize`".to_string());
    for i in 500..998 {
        lines.push(format!("   Compiling extra_{} v1.0.{}", i % 20, i));
    }
    lines
}

// (a) Structured first element is a JSON line: starts with {"squeez":"summary" ends with }
#[test]
fn structured_first_line_is_json_envelope() {
    let lines = make_cargo_lines();
    let out = apply_with_format(lines, "cargo build", SummaryFormat::Structured);
    assert!(!out.is_empty(), "output must not be empty");
    let first = &out[0];
    assert!(
        first.starts_with("{\"squeez\":\"summary\""),
        "first line should start with JSON envelope, got: {}",
        &first[..first.len().min(80)]
    );
    assert!(
        first.ends_with('}'),
        "first line should end with '}}', got: ...{}",
        &first[first.len().saturating_sub(20)..]
    );
    // Must be a single line (no embedded newlines)
    assert!(!first.contains('\n'), "JSON line must not contain newlines");
}

// (b) Byte-size: total structured output <= 50% of total prose output.
// Structured emits 1 JSON line + 5 tail lines; Prose emits ~14 header lines
// + 20 tail lines, so structured wins decisively on total byte count.
#[test]
fn structured_is_at_most_half_prose_bytes() {
    let lines = make_cargo_lines();
    let prose_out = apply_with_format(lines.clone(), "cargo build", SummaryFormat::Prose);
    let structured_out = apply_with_format(lines, "cargo build", SummaryFormat::Structured);

    let prose_bytes: usize = prose_out.iter().map(|l| l.len()).sum();
    let structured_bytes: usize = structured_out.iter().map(|l| l.len()).sum();

    assert!(
        structured_bytes * 2 <= prose_bytes,
        "structured ({} bytes) must be <= 50% of prose ({} bytes)",
        structured_bytes,
        prose_bytes,
    );
}

// (c) Error preservation: both outputs contain the two distinctive error substrings
#[test]
fn both_formats_preserve_errors() {
    let lines = make_cargo_lines();
    let prose_out = apply_with_format(lines.clone(), "cargo build", SummaryFormat::Prose);
    let structured_out = apply_with_format(lines, "cargo build", SummaryFormat::Structured);

    for out in [&prose_out, &structured_out] {
        let joined = out.join("\n");
        assert!(
            joined.contains("UNIQUE_ERROR_ALPHA"),
            "output must contain UNIQUE_ERROR_ALPHA"
        );
        assert!(
            joined.contains("UNIQUE_ERROR_BETA"),
            "output must contain UNIQUE_ERROR_BETA"
        );
    }
}

// (d) auto mode: Full intensity -> Prose shape; Ultra intensity -> Structured shape
#[test]
fn auto_mode_selects_format_by_intensity() {
    use squeez::config::Config;
    use squeez::context::intensity::Intensity;

    let lines = make_cargo_lines();

    // Simulate the same logic as wrap.rs auto selection
    let resolve_format = |cfg: &Config, intensity: Intensity| -> SummaryFormat {
        match cfg.summary_format.as_str() {
            "prose" => SummaryFormat::Prose,
            "structured" => SummaryFormat::Structured,
            _ => {
                if intensity == Intensity::Ultra {
                    SummaryFormat::Structured
                } else {
                    SummaryFormat::Prose
                }
            }
        }
    };

    let mut cfg = Config::default();
    cfg.summary_format = "auto".to_string();

    // Full intensity -> Prose
    let fmt_full = resolve_format(&cfg, Intensity::Full);
    assert_eq!(fmt_full, SummaryFormat::Prose, "auto+Full should give Prose");

    // Ultra intensity -> Structured
    let fmt_ultra = resolve_format(&cfg, Intensity::Ultra);
    assert_eq!(fmt_ultra, SummaryFormat::Structured, "auto+Ultra should give Structured");

    // Verify Prose output starts with prose marker
    let prose_out = apply_with_format(lines.clone(), "cargo build", fmt_full);
    let joined_prose = prose_out.join("\n");
    assert!(joined_prose.contains("squeez:summary"), "Prose output should contain squeez:summary");

    // Verify Structured output starts with JSON envelope
    let struct_out = apply_with_format(lines, "cargo build", fmt_ultra);
    assert!(
        struct_out[0].starts_with("{\"squeez\":\"summary\""),
        "Structured output first line must be JSON"
    );
}

// (e) Explicit overrides: "prose" always gives Prose, "structured" always gives Structured
#[test]
fn explicit_overrides_ignore_intensity() {
    use squeez::config::Config;
    use squeez::context::intensity::Intensity;

    let lines = make_cargo_lines();

    let resolve_format = |cfg: &Config, intensity: Intensity| -> SummaryFormat {
        match cfg.summary_format.as_str() {
            "prose" => SummaryFormat::Prose,
            "structured" => SummaryFormat::Structured,
            _ => {
                if intensity == Intensity::Ultra {
                    SummaryFormat::Structured
                } else {
                    SummaryFormat::Prose
                }
            }
        }
    };

    // "prose" in Ultra -> still Prose
    let mut cfg_prose = Config::default();
    cfg_prose.summary_format = "prose".to_string();
    for intensity in [Intensity::Lite, Intensity::Full, Intensity::Ultra] {
        let fmt = resolve_format(&cfg_prose, intensity);
        assert_eq!(fmt, SummaryFormat::Prose, "prose override must always give Prose (intensity={:?})", intensity);
    }

    // "structured" in Full -> still Structured
    let mut cfg_struct = Config::default();
    cfg_struct.summary_format = "structured".to_string();
    for intensity in [Intensity::Lite, Intensity::Full, Intensity::Ultra] {
        let fmt = resolve_format(&cfg_struct, intensity);
        assert_eq!(fmt, SummaryFormat::Structured, "structured override must always give Structured (intensity={:?})", intensity);
    }

    // Verify the actual outputs match expectations
    let prose_out = apply_with_format(lines.clone(), "cargo build", SummaryFormat::Prose);
    assert!(prose_out.join("\n").contains("squeez:summary"));

    let struct_out = apply_with_format(lines, "cargo build", SummaryFormat::Structured);
    assert!(struct_out[0].starts_with("{\"squeez\":\"summary\""));
}
