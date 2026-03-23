use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{smart_filter, truncation};

pub struct TypescriptHandler;

impl Handler for TypescriptHandler {
    fn compress(&self, _cmd: &str, lines: Vec<String>, _config: &Config) -> Vec<String> {
        let lines = smart_filter::apply(lines);
        let filtered: Vec<String> = lines
            .into_iter()
            .filter(|l| {
                let t = l.trim();
                l.contains("error TS")
                    || l.contains("warning TS")
                    || (l.contains(':') && l.contains('(') && l.contains(')'))
                    || t.starts_with("Found ")
                    || t.starts_with("error:")
            })
            .collect();
        truncation::apply(filtered, 100, truncation::Keep::Head)
    }
}
