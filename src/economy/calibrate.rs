use crate::config::Config;
use crate::session;
use std::path::Path;

// ── Language detection ────────────────────────────────────────────────────────

/// Detect the user's preferred language from the OS.
/// Priority: macOS AppleLanguages → $LANG / $LC_ALL → "en"
/// Returns a squeez lang code: "pt-BR", "en", etc.
pub fn detect_lang() -> String {
    // macOS: `defaults read -g AppleLanguages` → ("pt-BR", "en-US", ...)
    if let Ok(out) = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout);
        if let Some(lang) = parse_apple_languages(&s) {
            return lang;
        }
    }

    // POSIX: $LANG or $LC_ALL → "pt_BR.UTF-8" → "pt-BR"
    for var in &["LC_ALL", "LANG", "LANGUAGE"] {
        if let Ok(val) = std::env::var(var) {
            if !val.is_empty() && val != "C" && val != "C.UTF-8" && val != "POSIX" {
                return parse_posix_locale(&val);
            }
        }
    }

    "en".to_string()
}

/// Parse `defaults read -g AppleLanguages` output.
/// Input example: `(\n    "pt-BR",\n    "en-US",\n)`
fn parse_apple_languages(s: &str) -> Option<String> {
    for line in s.lines() {
        let trimmed = line.trim().trim_matches(',').trim_matches('"');
        if trimmed.is_empty() || trimmed == "(" || trimmed == ")" {
            continue;
        }
        return Some(normalize_lang(trimmed));
    }
    None
}

/// Parse POSIX locale string like "pt_BR.UTF-8" → "pt-BR", "en_US.UTF-8" → "en".
fn parse_posix_locale(s: &str) -> String {
    let base = s.split('.').next().unwrap_or(s);
    normalize_lang(&base.replace('_', "-"))
}

/// Normalize to squeez lang codes. Only pt-BR has a dedicated persona; all
/// others fall back to "en".
fn normalize_lang(s: &str) -> String {
    let lower = s.to_lowercase();
    if lower.starts_with("pt") {
        "pt-BR".to_string()
    } else {
        "en".to_string()
    }
}

/// Append `lang = <value>` to an existing config.ini file, unless already set.
fn write_lang_to_config(config_path: &str, lang: &str) {
    let existing = std::fs::read_to_string(config_path).unwrap_or_default();
    if existing.contains("lang =") || existing.contains("lang=") {
        return; // already set — preserve user value
    }
    if lang == "en" {
        return; // en is the default, no need to write explicitly
    }
    let appended = format!("{}\nlang = {}\n", existing.trim_end(), lang);
    let _ = std::fs::write(config_path, appended);
}

// ── Calibration profiles ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CalibrationProfile {
    pub max_lines: usize,
    pub dedup_min: usize,
    pub summarize_threshold_lines: usize,
    pub auto_compress_md: bool,
    pub ultra_trigger_pct: f32,
    pub adaptive_intensity: bool,
    pub read_max_lines: usize,
    pub grep_max_results: usize,
}

impl CalibrationProfile {
    fn aggressive() -> Self {
        Self {
            max_lines: 120,
            dedup_min: 2,
            summarize_threshold_lines: 300,
            auto_compress_md: true,
            ultra_trigger_pct: 0.70,
            adaptive_intensity: true,
            read_max_lines: 300,
            grep_max_results: 100,
        }
    }

    fn balanced() -> Self {
        Self {
            max_lines: 200,
            dedup_min: 3,
            summarize_threshold_lines: 500,
            auto_compress_md: true,
            ultra_trigger_pct: 0.80,
            adaptive_intensity: true,
            read_max_lines: 0,
            grep_max_results: 0,
        }
    }

    fn conservative() -> Self {
        Self {
            max_lines: 350,
            dedup_min: 5,
            summarize_threshold_lines: 800,
            auto_compress_md: false,
            ultra_trigger_pct: 0.90,
            adaptive_intensity: true,
            read_max_lines: 0,
            grep_max_results: 0,
        }
    }
}

// ── Heuristic selection ───────────────────────────────────────────────────────

/// Result of analyzing benchmark data.
#[derive(Debug, Clone)]
pub struct BenchmarkAnalysis {
    /// Average reduction percentage across all scenarios (0.0–100.0).
    pub avg_reduction_pct: f64,
    /// Number of scenarios that passed quality threshold.
    pub quality_pass_count: usize,
    /// Total number of scenarios.
    pub total_scenarios: usize,
}

/// Select calibration profile based on benchmark analysis.
pub fn select_profile(analysis: &BenchmarkAnalysis) -> CalibrationProfile {
    let all_pass = analysis.quality_pass_count == analysis.total_scenarios;
    if analysis.avg_reduction_pct > 70.0 && all_pass {
        CalibrationProfile::aggressive()
    } else if analysis.avg_reduction_pct > 50.0 {
        CalibrationProfile::balanced()
    } else {
        CalibrationProfile::conservative()
    }
}

/// Generate config.ini content from a calibration profile.
pub fn profile_to_config(profile: &CalibrationProfile) -> String {
    let mut lines = Vec::new();
    lines.push("# squeez config — auto-generated by `squeez calibrate`".to_string());
    lines.push(format!("max_lines = {}", profile.max_lines));
    lines.push(format!("dedup_min = {}", profile.dedup_min));
    lines.push(format!(
        "summarize_threshold_lines = {}",
        profile.summarize_threshold_lines
    ));
    lines.push(format!("auto_compress_md = {}", profile.auto_compress_md));
    lines.push(format!("ultra_trigger_pct = {:.2}", profile.ultra_trigger_pct));
    lines.push(format!("adaptive_intensity = {}", profile.adaptive_intensity));
    if profile.read_max_lines > 0 {
        lines.push(format!("read_max_lines = {}", profile.read_max_lines));
    }
    if profile.grep_max_results > 0 {
        lines.push(format!("grep_max_results = {}", profile.grep_max_results));
    }
    lines.join("\n") + "\n"
}

/// Write calibrated config.ini, backing up existing file.
pub fn write_config(profile: &CalibrationProfile) -> Result<String, String> {
    let base = std::env::var("SQUEEZ_DIR").unwrap_or_else(|_| {
        format!("{}/.claude/squeez", session::home_dir())
    });
    let config_path = format!("{}/config.ini", base);
    let path = Path::new(&config_path);

    // Backup existing
    if path.exists() {
        let bak = format!("{}/config.ini.bak", base);
        let _ = std::fs::copy(path, &bak);
    }

    let content = profile_to_config(profile);
    std::fs::write(path, &content).map_err(|e| format!("write config: {}", e))?;
    Ok(config_path)
}

// ── CLI entry point ───────────────────────────────────────────────────────────

/// Run calibration: execute benchmark, analyze results, write optimized config.
///
/// For now this runs a lightweight analysis of the existing benchmark fixtures
/// to determine the appropriate compression profile. Full benchmark integration
/// (calling benchmark::run internally) will be added in Phase 7.
pub fn run(args: &[String]) -> i32 {
    let force_aggressive = args.iter().any(|a| a == "--force-aggressive");

    if force_aggressive {
        eprintln!("squeez calibrate: applying aggressive profile (--force-aggressive)...");
        let lang = detect_lang();
        eprintln!("  Language detected: {}", lang);
        let profile = CalibrationProfile::aggressive();
        match write_config(&profile) {
            Ok(path) => {
                write_lang_to_config(&path, &lang);
                eprintln!("  Config written: {}", path);
                return 0;
            }
            Err(e) => { eprintln!("  Error: {}", e); return 1; }
        }
    }

    eprintln!("squeez calibrate: analyzing compression characteristics...");

    // Run a quick analysis: load config defaults, try compressing a sample,
    // and select profile based on the built-in benchmark characteristics.
    let analysis = quick_analysis();

    let profile = select_profile(&analysis);
    let tier = if analysis.avg_reduction_pct > 70.0
        && analysis.quality_pass_count == analysis.total_scenarios
    {
        "aggressive"
    } else if analysis.avg_reduction_pct > 50.0 {
        "balanced"
    } else {
        "conservative"
    };

    eprintln!(
        "  Profile: {} (avg reduction: {:.1}%, quality: {}/{})",
        tier,
        analysis.avg_reduction_pct,
        analysis.quality_pass_count,
        analysis.total_scenarios,
    );

    let lang = detect_lang();
    eprintln!("  Language detected: {}", lang);
    match write_config(&profile) {
        Ok(path) => {
            write_lang_to_config(&path, &lang);
            eprintln!("  Config written: {}", path);
            0
        }
        Err(e) => {
            eprintln!("  Error: {}", e);
            1
        }
    }
}

/// Quick analysis without running full benchmark — uses a synthetic sample
/// to test compression pipeline and estimate reduction.
fn quick_analysis() -> BenchmarkAnalysis {
    let cfg = Config::load();

    // Generate synthetic test data and compress it.
    let sample = generate_test_output();
    let lines: Vec<String> = sample.lines().map(String::from).collect();
    let original_len = lines.len();

    let compressed = crate::filter::compress("git status", lines, &cfg);
    let compressed_len = compressed.len();

    let reduction = if original_len > 0 {
        ((original_len - compressed_len) as f64 / original_len as f64) * 100.0
    } else {
        0.0
    };

    // Quality check: key signal lines should survive compression.
    let quality_pass = compressed.iter().any(|l| l.contains("modified:"))
        || compressed.iter().any(|l| l.contains("Changes"));

    BenchmarkAnalysis {
        avg_reduction_pct: reduction,
        quality_pass_count: if quality_pass { 1 } else { 0 },
        total_scenarios: 1,
    }
}

/// Generate a synthetic git status output for calibration testing.
fn generate_test_output() -> String {
    let mut lines = Vec::new();
    lines.push("On branch main".to_string());
    lines.push("Your branch is up to date with 'origin/main'.".to_string());
    lines.push("".to_string());
    lines.push("Changes not staged for commit:".to_string());
    lines.push("  (use \"git add <file>...\" to update what will be committed)".to_string());
    for i in 0..30 {
        lines.push(format!("\tmodified:   src/file_{}.rs", i));
    }
    lines.push("".to_string());
    lines.push("Untracked files:".to_string());
    for i in 0..20 {
        lines.push(format!("\tnew_file_{}.rs", i));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggressive_profile_selection() {
        let analysis = BenchmarkAnalysis {
            avg_reduction_pct: 75.0,
            quality_pass_count: 10,
            total_scenarios: 10,
        };
        let p = select_profile(&analysis);
        assert_eq!(p.max_lines, 120);
        assert_eq!(p.dedup_min, 2);
    }

    #[test]
    fn balanced_profile_selection() {
        let analysis = BenchmarkAnalysis {
            avg_reduction_pct: 60.0,
            quality_pass_count: 8,
            total_scenarios: 10,
        };
        let p = select_profile(&analysis);
        assert_eq!(p.max_lines, 200);
    }

    #[test]
    fn conservative_profile_selection() {
        let analysis = BenchmarkAnalysis {
            avg_reduction_pct: 25.0,
            quality_pass_count: 5,
            total_scenarios: 10,
        };
        let p = select_profile(&analysis);
        assert_eq!(p.max_lines, 350);
        assert_eq!(p.dedup_min, 5);
    }

    #[test]
    fn config_generation() {
        let p = CalibrationProfile::aggressive();
        let ini = profile_to_config(&p);
        assert!(ini.contains("max_lines = 120"));
        assert!(ini.contains("dedup_min = 2"));
        assert!(ini.contains("read_max_lines = 300"));
    }

    #[test]
    fn balanced_config_omits_zero_budgets() {
        let p = CalibrationProfile::balanced();
        let ini = profile_to_config(&p);
        assert!(!ini.contains("read_max_lines"));
        assert!(!ini.contains("grep_max_results"));
    }
}
