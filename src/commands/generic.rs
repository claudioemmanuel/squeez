use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{dedup, log_template, smart_filter, truncation};

pub struct GenericHandler;

impl Handler for GenericHandler {
    fn compress(&self, cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        // No grouping here: the generic handler sees arbitrary command output
        // where the lines ARE the payload (prose, logs, anything). Collapsing
        // them to "N modified" destroys content and can swallow errors (#165,
        // same principle as #148). dedup + log-template + relevance-truncation
        // do the compression; grouping stays reserved for git status output.
        let lines = smart_filter::apply(lines);
        let lines = dedup::apply(lines, config.dedup_min);
        let lines = if config.log_template_enabled {
            log_template::apply(lines, config.log_template_min)
        } else {
            lines
        };
        if config.relevance_truncation_enabled {
            truncation::apply_relevant(lines, config.max_lines, cmd)
        } else {
            truncation::apply(lines, config.max_lines, truncation::Keep::Head)
        }
    }
}
