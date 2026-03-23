use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{smart_filter, truncation};

pub struct RuntimeHandler;

impl Handler for RuntimeHandler {
    fn compress(&self, _cmd: &str, lines: Vec<String>, _config: &Config) -> Vec<String> {
        let lines = smart_filter::apply(lines);
        let filtered: Vec<String> = lines
            .into_iter()
            .filter(|l| {
                !(l.trim_start().starts_with("at internal/")
                    || l.trim_start().starts_with("at async internal/"))
            })
            .collect();
        truncation::apply(filtered, 50, truncation::Keep::Head)
    }
}
