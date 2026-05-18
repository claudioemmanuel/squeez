//! Enterprise-deployment awareness.
//!
//! On Anthropic's usage-based enterprise plans there is **no** included token
//! allowance — every token is billed at API rates and an admin-set spend cap
//! pauses the workspace when hit, requiring a manual refill ticket. In that
//! regime, the bytes squeez removes convert directly to USD and to runway
//! before the next budget request.
//!
//! This module detects the deployment transport (Bedrock, Vertex, or an OTEL
//! metrics pipe — all strong signals that an operator wants cost visibility)
//! and provides a small USD-savings estimator that callers can quote in the
//! wrap header, the init banner, and the `squeez_enterprise_savings` MCP
//! tool.
//!
//! Detection is **env-var only** — we never reach out to the network and we
//! never read user credentials. All inputs are public process env.
//!
//! Reference: <https://code.claude.com/docs/en/costs>

/// What transport / cost-visibility regime is the host running in?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnterpriseMode {
    /// No enterprise signal — local Pro/Max/Team subscription.
    None,
    /// AWS Bedrock-backed Claude (`CLAUDE_CODE_USE_BEDROCK=1`).
    Bedrock,
    /// Google Vertex-backed Claude (`CLAUDE_CODE_USE_VERTEX=1`).
    Vertex,
    /// OTEL metric exporter wired — operator is tracking spend.
    Otel,
}

impl EnterpriseMode {
    /// Short stable slug for use in headers and JSON.
    pub fn slug(self) -> &'static str {
        match self {
            EnterpriseMode::None => "none",
            EnterpriseMode::Bedrock => "bedrock",
            EnterpriseMode::Vertex => "vertex",
            EnterpriseMode::Otel => "otel",
        }
    }

    /// Was any enterprise transport detected?
    pub fn is_enterprise(self) -> bool {
        !matches!(self, EnterpriseMode::None)
    }
}

/// Detect the enterprise mode from process env vars.
///
/// Precedence (highest wins): Bedrock → Vertex → OTEL → none. Bedrock and
/// Vertex are explicit transport choices and stronger signals than mere
/// OTEL telemetry.
pub fn detect() -> EnterpriseMode {
    detect_from(&std::env::vars().collect::<Vec<_>>())
}

/// Pure variant for tests — pass an explicit env list, get back the mode.
pub fn detect_from<S: AsRef<str>>(env: &[(S, S)]) -> EnterpriseMode {
    let lookup = |key: &str| -> Option<&str> {
        env.iter()
            .find(|(k, _)| k.as_ref().eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_ref())
    };
    let truthy = |v: &str| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes");

    if lookup("CLAUDE_CODE_USE_BEDROCK").map(truthy).unwrap_or(false) {
        return EnterpriseMode::Bedrock;
    }
    if lookup("CLAUDE_CODE_USE_VERTEX").map(truthy).unwrap_or(false) {
        return EnterpriseMode::Vertex;
    }
    if lookup("ANTHROPIC_VERTEX_PROJECT_ID").is_some() {
        return EnterpriseMode::Vertex;
    }
    if lookup("AWS_BEDROCK_REGION").is_some() {
        return EnterpriseMode::Bedrock;
    }
    if lookup("OTEL_EXPORTER_OTLP_ENDPOINT").is_some()
        || lookup("OTEL_METRIC_EXPORT_INTERVAL").is_some()
        || lookup("CLAUDE_CODE_ENABLE_TELEMETRY")
            .map(truthy)
            .unwrap_or(false)
    {
        return EnterpriseMode::Otel;
    }
    EnterpriseMode::None
}

/// Default-target pricing model for savings estimates. We default to the
/// cheaper of the two (Sonnet) so estimates are conservative — never
/// overstate value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PricingModel {
    /// Claude Sonnet 4.6 — \$3.00 / MTok input. Default.
    Sonnet46,
    /// Claude Opus 4.7 — \$5.00 / MTok input.
    Opus47,
}

impl PricingModel {
    /// Input price in USD per 1,000,000 tokens.
    pub fn input_usd_per_mtok(self) -> f64 {
        match self {
            PricingModel::Sonnet46 => 3.0,
            PricingModel::Opus47 => 5.0,
        }
    }
}

/// Estimate USD value of the given saved input-token count.
///
/// Conservative: uses Anthropic's public list price for input tokens. Real
/// enterprise contracts often have negotiated discounts, so this is an
/// upper bound on the discount realised but a fair *floor* on the savings
/// claim relative to on-demand pricing.
pub fn estimate_usd(saved_input_tokens: u64, model: PricingModel) -> f64 {
    (saved_input_tokens as f64) * model.input_usd_per_mtok() / 1_000_000.0
}

/// Render the wrap-header tag for the detected mode, e.g.
/// `[squeez: enterprise=bedrock]`. Empty string when not in enterprise mode.
pub fn header_tag(mode: EnterpriseMode) -> String {
    if !mode.is_enterprise() {
        return String::new();
    }
    format!("[squeez: enterprise={}]", mode.slug())
}

/// Render a single-line init banner line. Empty when no enterprise signal.
pub fn init_banner_line(mode: EnterpriseMode, prior_session_saved_usd: f64) -> String {
    if !mode.is_enterprise() {
        return String::new();
    }
    if prior_session_saved_usd > 0.0 {
        format!(
            "Enterprise mode ({}): squeez saved an estimated ${:.4} in the prior session.",
            mode.slug(),
            prior_session_saved_usd,
        )
    } else {
        format!(
            "Enterprise mode ({}): every saved token converts to USD on this transport.",
            mode.slug()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn detects_bedrock_via_use_flag() {
        let e = env(&[("CLAUDE_CODE_USE_BEDROCK", "1")]);
        assert_eq!(detect_from(&e), EnterpriseMode::Bedrock);
    }

    #[test]
    fn detects_bedrock_via_region_var() {
        let e = env(&[("AWS_BEDROCK_REGION", "us-east-1")]);
        assert_eq!(detect_from(&e), EnterpriseMode::Bedrock);
    }

    #[test]
    fn detects_vertex_via_use_flag() {
        let e = env(&[("CLAUDE_CODE_USE_VERTEX", "true")]);
        assert_eq!(detect_from(&e), EnterpriseMode::Vertex);
    }

    #[test]
    fn detects_vertex_via_project_id() {
        let e = env(&[("ANTHROPIC_VERTEX_PROJECT_ID", "my-project")]);
        assert_eq!(detect_from(&e), EnterpriseMode::Vertex);
    }

    #[test]
    fn detects_otel_via_endpoint() {
        let e = env(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "http://collector:4318")]);
        assert_eq!(detect_from(&e), EnterpriseMode::Otel);
    }

    #[test]
    fn bedrock_beats_otel_when_both_set() {
        let e = env(&[
            ("CLAUDE_CODE_USE_BEDROCK", "1"),
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://x"),
        ]);
        assert_eq!(detect_from(&e), EnterpriseMode::Bedrock);
    }

    #[test]
    fn empty_env_means_no_enterprise() {
        let e: Vec<(String, String)> = Vec::new();
        assert_eq!(detect_from(&e), EnterpriseMode::None);
    }

    #[test]
    fn falsy_use_flag_is_not_enterprise() {
        let e = env(&[("CLAUDE_CODE_USE_BEDROCK", "0")]);
        assert_eq!(detect_from(&e), EnterpriseMode::None);
        let e = env(&[("CLAUDE_CODE_USE_BEDROCK", "")]);
        assert_eq!(detect_from(&e), EnterpriseMode::None);
    }

    #[test]
    fn pricing_math_matches_published_rates() {
        // 1 MTok at Sonnet should be exactly $3.00
        assert!((estimate_usd(1_000_000, PricingModel::Sonnet46) - 3.0).abs() < 1e-9);
        // 1 MTok at Opus should be exactly $5.00
        assert!((estimate_usd(1_000_000, PricingModel::Opus47) - 5.0).abs() < 1e-9);
        // 1000 tokens at Sonnet = $0.003
        assert!((estimate_usd(1_000, PricingModel::Sonnet46) - 0.003).abs() < 1e-9);
    }

    #[test]
    fn header_tag_empty_when_not_enterprise() {
        assert!(header_tag(EnterpriseMode::None).is_empty());
        assert_eq!(header_tag(EnterpriseMode::Bedrock), "[squeez: enterprise=bedrock]");
    }

    #[test]
    fn init_banner_changes_with_prior_savings() {
        let with = init_banner_line(EnterpriseMode::Bedrock, 0.1234);
        assert!(with.contains("$0.1234"), "{}", with);
        let without = init_banner_line(EnterpriseMode::Bedrock, 0.0);
        assert!(without.contains("Enterprise mode (bedrock)"));
        assert!(init_banner_line(EnterpriseMode::None, 0.0).is_empty());
    }
}
