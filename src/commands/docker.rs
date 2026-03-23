use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{dedup, smart_filter, truncation};

pub struct DockerHandler;

impl Handler for DockerHandler {
    fn compress(&self, _cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        let lines = smart_filter::apply(lines);
        let lines = dedup::apply(lines, config.dedup_min);
        truncation::apply(lines, config.docker_logs_max_lines, truncation::Keep::Tail)
    }
}
