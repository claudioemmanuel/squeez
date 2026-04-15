use crate::config::Config;

// ── Budget params ─────────────────────────────────────────────────────────────

/// Returns a JSON patch string to inject into tool_input for the given tool,
/// enforcing configured output size budgets.
///
/// Returns `None` if the budget for the tool is 0 (disabled) or the tool
/// is not handled.
pub fn budget_params(tool_name: &str, cfg: &Config) -> Option<String> {
    match tool_name {
        "Read" if cfg.read_max_lines > 0 => {
            Some(format!("{{\"limit\":{}}}", cfg.read_max_lines))
        }
        "Grep" if cfg.grep_max_results > 0 => {
            Some(format!("{{\"head_limit\":{}}}", cfg.grep_max_results))
        }
        _ => None,
    }
}

/// CLI entry point: `squeez budget-params <tool>`
/// Prints JSON patch to stdout if budget is configured, exits 0.
pub fn run(args: &[String]) -> i32 {
    let tool = match args.first() {
        Some(t) => t.as_str(),
        None => {
            eprintln!("squeez budget-params: no tool name given");
            return 1;
        }
    };
    let cfg = Config::load();
    if let Some(json) = budget_params(tool, &cfg) {
        println!("{}", json);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_budget_enabled_by_default() {
        let cfg = Config::default();
        // Default is aggressive profile: read_max_lines = 300
        let json = budget_params("Read", &cfg).unwrap();
        assert_eq!(json, "{\"limit\":300}");
    }

    #[test]
    fn read_budget_disabled_when_zero() {
        let mut cfg = Config::default();
        cfg.read_max_lines = 0;
        assert!(budget_params("Read", &cfg).is_none());
    }

    #[test]
    fn read_budget_enabled() {
        let mut cfg = Config::default();
        cfg.read_max_lines = 500;
        let json = budget_params("Read", &cfg).unwrap();
        assert_eq!(json, "{\"limit\":500}");
    }

    #[test]
    fn grep_budget_enabled() {
        let mut cfg = Config::default();
        cfg.grep_max_results = 100;
        let json = budget_params("Grep", &cfg).unwrap();
        assert_eq!(json, "{\"head_limit\":100}");
    }

    #[test]
    fn unknown_tool_returns_none() {
        let cfg = Config::default();
        assert!(budget_params("Edit", &cfg).is_none());
        assert!(budget_params("Bash", &cfg).is_none());
    }
}
