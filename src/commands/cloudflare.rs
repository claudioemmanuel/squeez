use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{dedup, smart_filter, truncation};

pub struct CloudflareHandler;

impl Handler for CloudflareHandler {
    fn compress(&self, _cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        let lines = smart_filter::apply(lines);
        let filtered: Vec<String> = lines
            .into_iter()
            .filter(|l| {
                let t = l.trim();
                if t.is_empty() {
                    return false;
                }
                // Drop wrangler version banner and decorative separators
                if t.starts_with("⛅") || t.starts_with('\u{26c5}') {
                    return false;
                }
                if t.chars().all(|c| c == '─' || c == '-' || c == '=' || c == ' ') {
                    return false;
                }
                // Drop boilerplate preamble lines
                if t.starts_with("Retrieving cached") {
                    return false;
                }
                if t.starts_with("Your worker has access to the following bindings") {
                    return false;
                }
                // Drop individual binding detail lines (indented with "- " under bindings)
                if t.starts_with("- KV:")
                    || t.starts_with("- D1:")
                    || t.starts_with("- R2:")
                    || t.starts_with("- DO:")
                    || t.starts_with("- AI:")
                    || t.starts_with("- Queue:")
                    || t.starts_with("- Vectorize:")
                {
                    return false;
                }
                // Drop update-available notices
                if t.contains("update available") || t.contains("npm install wrangler") {
                    return false;
                }
                true
            })
            .collect();
        let filtered = dedup::apply(filtered, config.dedup_min);
        truncation::apply(filtered, 60, truncation::Keep::Tail)
    }
}
