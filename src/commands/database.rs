use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{smart_filter, truncation};

pub struct DatabaseHandler;

impl Handler for DatabaseHandler {
    fn compress(&self, _cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        let lines = smart_filter::apply(lines);
        let filtered: Vec<String> = lines
            .into_iter()
            .filter(|l| !l.starts_with('+') && !l.trim().is_empty())
            .collect();
        truncation::apply(
            filtered,
            config.find_max_results + 2,
            truncation::Keep::Head,
        )
    }
}
