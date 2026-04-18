use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{dedup, smart_filter, truncation};

pub struct WranglerHandler;

impl Handler for WranglerHandler {
    fn compress(&self, _cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        let lines = smart_filter::apply(lines);
        let filtered: Vec<String> = lines
            .into_iter()
            .filter(|l| {
                let t = l.trim();
                if t.is_empty() {
                    return false;
                }
                // Drop wrangler upload/bundle progress + branding chrome.
                let noise = [
                    "⛅️ wrangler",
                    "-------------------",
                    "Total Upload:",
                    "Worker Startup Time:",
                    "No bindings found.",
                    "Your Worker has access to the following bindings:",
                    "Proxy server listening",
                    "Ready on http://",
                ];
                if noise.iter().any(|p| t.starts_with(p)) {
                    return false;
                }
                // Drop file-upload progress lines: "  src/index.ts   1.2 KiB / gzip: 0.5 KiB".
                if t.contains(" KiB / gzip:") || t.contains(" MiB / gzip:") {
                    return false;
                }
                // Drop spinner / progress chars.
                if t.starts_with('⠋')
                    || t.starts_with('⠙')
                    || t.starts_with('⠹')
                    || t.starts_with('⠸')
                    || t.starts_with('⠼')
                {
                    return false;
                }
                true
            })
            .collect();
        let filtered = dedup::apply(filtered, config.dedup_min);
        // Tail — error + deploy-URL summary is always at the end.
        truncation::apply(filtered, 80, truncation::Keep::Tail)
    }
}
