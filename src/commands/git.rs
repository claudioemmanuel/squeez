use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{dedup, grouping, smart_filter, truncation};

pub struct GitHandler;

impl Handler for GitHandler {
    fn compress(&self, cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        let lines = smart_filter::apply(lines);
        let lines = dedup::apply(lines, config.dedup_min);

        if cmd.contains("log") {
            return truncation::apply(lines, config.git_log_max_commits, truncation::Keep::Head);
        }
        if cmd.contains("diff") {
            return truncation::apply(lines, config.git_diff_max_lines, truncation::Keep::Head);
        }
        // status, branch, etc.
        let lines = grouping::group_files_by_dir(lines, 4);
        truncation::apply(lines, 60, truncation::Keep::Head)
    }
}
