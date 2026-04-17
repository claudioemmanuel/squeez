use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{dedup, smart_filter, truncation};

pub struct NextjsHandler;

impl Handler for NextjsHandler {
    fn compress(&self, cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        let lines = smart_filter::apply(lines);

        let is_build = cmd.contains("build");

        let filtered: Vec<String> = lines
            .into_iter()
            .filter(|l| {
                let t = l.trim();
                if t.is_empty() {
                    return false;
                }
                // Drop the Next.js version banner line (▲ Next.js X.Y.Z)
                if t.starts_with('▲') && t.contains("Next.js") && !t.contains("Error") {
                    return false;
                }
                // Drop routine compilation progress lines when not a build
                if !is_build && t.starts_with("○ Compiling") {
                    return false;
                }
                // Drop verbose module counts from compiled lines
                // e.g. "✓ Compiled / in 234ms (500 modules)" → keep
                // "  - Environments: .env" → drop boilerplate
                if t.starts_with("- Environments:") {
                    return false;
                }
                // Drop "- Network: ..." dev-server info (redundant with Local)
                if t.starts_with("- Network:") {
                    return false;
                }
                true
            })
            .collect();

        let filtered = dedup::apply(filtered, config.dedup_min);

        // For builds prefer tail (errors appear at end); for dev keep head (URL first)
        if is_build {
            truncation::apply(filtered, 80, truncation::Keep::Tail)
        } else {
            truncation::apply(filtered, 60, truncation::Keep::Head)
        }
    }
}
