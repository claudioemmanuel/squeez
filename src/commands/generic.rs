use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{dedup, grouping, smart_filter, truncation};

pub struct GenericHandler;

impl Handler for GenericHandler {
    fn compress(&self, _cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        let lines = smart_filter::apply(lines);
        let lines = dedup::apply(lines, config.dedup_min);
        let lines = grouping::group_files_by_dir(lines, 5);
        truncation::apply(lines, config.max_lines, truncation::Keep::Head)
    }
}
