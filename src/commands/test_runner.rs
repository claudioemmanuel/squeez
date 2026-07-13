use crate::commands::reporters;
use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{dedup, smart_filter, truncation};

pub struct TestRunnerHandler;

impl Handler for TestRunnerHandler {
    fn compress(&self, cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        // Structured reporters need the raw shape (blank lines are structural
        // delimiters, not noise) — try them before smart_filter strips anything.
        if let Some(condensed) = reporters::detect_and_condense(cmd, &lines) {
            return condensed;
        }
        let lines = smart_filter::apply(lines);
        let filtered: Vec<String> = lines
            .into_iter()
            .filter(|l| {
                let is_passing = (l.contains('✓') || l.contains("✔") || l.contains(" ok"))
                    && !l.contains("failed")
                    && !l.contains("FAIL");
                !is_passing
            })
            .collect();
        let filtered = dedup::apply(filtered, config.dedup_min);
        truncation::apply(filtered, 100, truncation::Keep::Tail)
    }
}
