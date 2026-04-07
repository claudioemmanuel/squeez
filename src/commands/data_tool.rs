// Handler for JSON/YAML/IaC tools: jq, yq, terraform, helm, pulumi.
// These produce structured output that benefits from key extraction and
// truncation rather than the generic dedup/grouping pipeline.

use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{smart_filter, truncation};

pub struct DataToolHandler;

impl Handler for DataToolHandler {
    fn compress(&self, cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        let name = cmd.split_whitespace().next().unwrap_or("").to_lowercase();
        let name = name.rsplit('/').next().unwrap_or(&name);
        match name {
            "terraform" => compress_terraform(lines, config),
            "helm" => compress_helm(lines, config),
            _ => compress_json_yaml(lines, config),
        }
    }
}

/// Terraform plan/apply output: strip unchanged resources, keep add/change/destroy summary.
fn compress_terraform(lines: Vec<String>, config: &Config) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut in_unchanged = false;

    for line in &lines {
        let t = line.trim();
        // Skip noise lines
        if t.is_empty()
            || t.starts_with("Refreshing state...")
            || t.starts_with("Reading...")
            || (t.starts_with('#') && t.contains("is up-to-date"))
            || t == "Terraform will perform the following actions:"
            || t.starts_with("  # ") && t.ends_with(" (no changes)")
        {
            continue;
        }
        // Detect "# resource unchanged" blocks to skip
        if t.starts_with("  # ") && t.contains(" will be read") {
            in_unchanged = true;
            continue;
        }
        if in_unchanged {
            if t.is_empty() || t == "}" {
                in_unchanged = false;
            }
            continue;
        }
        // Keep important lines: +/-/~, Plan:, Apply complete!, Error, Warning
        let keep = t.starts_with("+ ")
            || t.starts_with("- ")
            || t.starts_with("~ ")
            || t.starts_with('+')
            || t.starts_with('-')
            || t.starts_with('~')
            || t.starts_with("Plan:")
            || t.starts_with("Apply complete!")
            || t.starts_with("Destroy complete!")
            || t.starts_with("Error:")
            || t.starts_with("Warning:")
            || t.contains("changes to perform")
            || t.contains("will be created")
            || t.contains("will be destroyed")
            || t.contains("will be updated");
        if keep {
            out.push(line.clone());
        }
    }

    // Always preserve the last summary line if any (Plan: N to add...)
    let limit = config.max_lines.max(20);
    truncation::apply(out, limit, truncation::Keep::Head)
}

/// Helm output: strip chart metadata boilerplate, keep resource lines.
fn compress_helm(lines: Vec<String>, config: &Config) -> Vec<String> {
    let filtered: Vec<String> = lines
        .into_iter()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty()
                && !t.starts_with("CHART:")
                && !t.starts_with("LAST DEPLOYED:")
                && !t.starts_with("NAMESPACE:")
                && !t.starts_with("STATUS:")
                && !t.starts_with("REVISION:")
                && !t.starts_with("TEST SUITE:")
                && !t.starts_with("---")
                && !t.starts_with("# Source:")
        })
        .collect();
    let filtered = smart_filter::apply(filtered);
    truncation::apply(filtered, config.max_lines.max(20), truncation::Keep::Head)
}

/// jq/yq: already compact JSON/YAML; apply smart_filter + truncation.
fn compress_json_yaml(lines: Vec<String>, config: &Config) -> Vec<String> {
    let filtered: Vec<String> = lines
        .into_iter()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && t != "{}" && t != "[]" && t != "null"
        })
        .collect();
    truncation::apply(filtered, config.max_lines.max(20), truncation::Keep::Head)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn terraform_drops_unchanged_and_keeps_plan_line() {
        let input: Vec<String> = vec![
            "Refreshing state...".into(),
            "  # aws_s3_bucket.x is up-to-date".into(),
            "+ resource \"aws_instance\" \"web\" {".into(),
            "    ami = \"ami-123\"".into(),
            "}".into(),
            "Plan: 1 to add, 0 to change, 0 to destroy.".into(),
        ];
        let cfg = Config::default();
        let out = DataToolHandler.compress("terraform plan", input, &cfg);
        assert!(out.iter().any(|l| l.contains("Plan:")));
        assert!(out.iter().any(|l| l.contains("aws_instance")));
        assert!(!out.iter().any(|l| l.contains("Refreshing")));
    }

    #[test]
    fn jq_drops_empty_lines() {
        let input: Vec<String> = vec![
            "{".into(),
            "  \"key\": \"value\"".into(),
            "}".into(),
            "{}".into(),
            "null".into(),
        ];
        let cfg = Config::default();
        let out = DataToolHandler.compress("jq .", input, &cfg);
        assert!(!out.iter().any(|l| l.trim() == "{}"));
        assert!(!out.iter().any(|l| l.trim() == "null"));
    }
}
