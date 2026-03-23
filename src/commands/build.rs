use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{dedup, smart_filter, truncation};

pub struct BuildHandler;

impl Handler for BuildHandler {
    fn compress(&self, _cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        let lines = smart_filter::apply(lines);
        let filtered: Vec<String> = lines
            .into_iter()
            .filter(|l| {
                !l.starts_with("> Task :")
                    && !l.starts_with("Executing ")
                    && !l.starts_with("Download ")
                    && !l.trim().is_empty()
            })
            .collect();
        let filtered = dedup::apply(filtered, config.dedup_min);
        truncation::apply(filtered, 100, truncation::Keep::Tail)
    }
}
