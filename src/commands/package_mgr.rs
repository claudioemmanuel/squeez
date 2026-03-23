use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{dedup, smart_filter, truncation};

pub struct PackageMgrHandler;

impl Handler for PackageMgrHandler {
    fn compress(&self, _cmd: &str, lines: Vec<String>, _config: &Config) -> Vec<String> {
        let lines = smart_filter::apply(lines);
        let lines = dedup::apply(lines, 2);
        truncation::apply(lines, 50, truncation::Keep::Tail)
    }
}
