use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{smart_filter, truncation};

pub struct CloudHandler;

impl Handler for CloudHandler {
    fn compress(&self, _cmd: &str, lines: Vec<String>, _config: &Config) -> Vec<String> {
        let lines = smart_filter::apply(lines);
        let filtered: Vec<String> = lines
            .into_iter()
            .filter(|l| !l.starts_with("---") && !l.trim().is_empty())
            .collect();
        truncation::apply(filtered, 100, truncation::Keep::Head)
    }
}
