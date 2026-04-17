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
                if t.is_empty() {
                    return false;
                }
                // Drop passing test lines. Playwright list reporter:
                //   "  ✓  1 [chromium] › foo.spec.ts:12:3 › renders (1.2s)"
                // Keep failures (✘ / ✗ / ×) and summary lines.
                let is_pass = (t.starts_with('✓') || t.starts_with("ok ") || t.contains(" ✓ "))
                    && !t.contains("failed")
                    && !t.contains("Error");
                if is_pass {
                    return false;
                }
                // Drop per-test progress dots + interim "Running N tests" banners.
                if t == "." || t.chars().all(|c| c == '.' || c == 'F' || c == 'T') {
                    return false;
                }
                // Drop trace/snapshot artifact paths — they're reproducible on demand.
                if t.starts_with("attachment #")
                    || t.starts_with("Slow test file:")
                    || t.starts_with("To open last HTML report")
                {
                    return false;
                }
                true
            })
            .collect();
        let filtered = dedup::apply(filtered, config.dedup_min);
        // Tail — final summary + stack traces of last failures are most actionable.
        truncation::apply(filtered, 120, truncation::Keep::Tail)
    }
}
