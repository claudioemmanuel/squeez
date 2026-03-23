use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{grouping, smart_filter, truncation};

pub struct FsHandler;

const NOISY_ENV_PREFIXES: &[&str] = &[
    "PATH=",
    "LS_COLORS=",
    "TERM=",
    "PS1=",
    "PROMPT=",
    "MANPATH=",
];

impl Handler for FsHandler {
    fn compress(&self, cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        let lines = smart_filter::apply(lines);

        if cmd.trim_start().starts_with("env") || cmd.contains("printenv") {
            let filtered: Vec<String> = lines
                .into_iter()
                .filter(|l| !NOISY_ENV_PREFIXES.iter().any(|p| l.starts_with(p)))
                .collect();
            return truncation::apply(filtered, 80, truncation::Keep::Head);
        }

        let lines = grouping::group_files_by_dir(lines, 5);
        truncation::apply(lines, config.find_max_results, truncation::Keep::Head)
    }
}
