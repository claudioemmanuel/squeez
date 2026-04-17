use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{dedup, smart_filter, truncation};

pub struct PlaywrightHandler;

impl Handler for PlaywrightHandler {
    fn compress(&self, _cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        let lines = smart_filter::apply(lines);

        let filtered: Vec<String> = lines
            .into_iter()
            .filter(|l| {
                let t = l.trim();
                // Drop passing test lines — keep failures, errors, and summaries
                let is_passing = (t.starts_with('✓') || t.starts_with('·'))
                    && !t.contains("fail")
                    && !t.contains("Error");
                if is_passing {
                    return false;
                }
                // Drop browser launch / teardown noise
                if t.starts_with("[chromium]") || t.starts_with("[firefox]") || t.starts_with("[webkit]") {
                    // Keep only failure lines (contain ✗ or "fail")
                    if !t.contains('✗') && !t.contains("fail") && !t.contains("Error") {
                        return false;
                    }
                }
                // Drop screenshot/video attachment lines for passing tests
                if t.contains("screenshot") && !t.contains("fail") && !t.contains("Error") {
                    return false;
                }
                true
            })
            .collect();

        let filtered = dedup::apply(filtered, config.dedup_min);
        // Tail: errors and summary appear at the end of playwright output
        truncation::apply(filtered, 100, truncation::Keep::Tail)
    }
}
