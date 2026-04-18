use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{dedup, smart_filter, truncation};

pub struct NextBuildHandler;

impl Handler for NextBuildHandler {
    fn compress(&self, _cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        let lines = smart_filter::apply(lines);
        let filtered: Vec<String> = lines
            .into_iter()
            .filter(|l| {
                let t = l.trim();
                if t.is_empty() {
                    return false;
                }
                // Next.js emits large chunks of branding + telemetry + static-gen
                // progress that carry no actionable signal.
                let noise = [
                    "▲ Next.js",
                    "- Experiments (use with caution)",
                    "Attention: Next.js now collects",
                    "https://nextjs.org/telemetry",
                    "Creating an optimized production build",
                    "Compiled successfully",
                    "Collecting page data",
                    "Generating static pages",
                    "Finalizing page optimization",
                    "Collecting build traces",
                ];
                if noise.iter().any(|p| t.starts_with(p)) {
                    return false;
                }
                // Spinner progress "(12/240)".
                if t.starts_with('(')
                    && t.contains('/')
                    && t.ends_with(')')
                    && t.chars().all(|c| c.is_ascii_digit() || c == '/' || c == '(' || c == ')')
                {
                    return false;
                }
                true
            })
            .collect();
        let filtered = dedup::apply(filtered, config.dedup_min);
        // Keep head: errors surface before the route table; if no errors,
        // the route table itself is the most useful artefact.
        truncation::apply(filtered, 140, truncation::Keep::Head)
    }
}
