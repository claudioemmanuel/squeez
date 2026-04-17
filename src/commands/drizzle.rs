use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{dedup, smart_filter, truncation};

pub struct DrizzleHandler;

impl Handler for DrizzleHandler {
    fn compress(&self, _cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        let lines = smart_filter::apply(lines);

        let filtered: Vec<String> = lines
            .into_iter()
            .filter(|l| {
                let t = l.trim();
                if t.is_empty() {
                    return false;
                }
                // Drop drizzle-kit version/banner line
                if t.starts_with("drizzle-kit:") && t.contains("v") && !t.contains("Error") {
                    return false;
                }
                // Drop config-reading boilerplate
                if t.starts_with("Reading config file") {
                    return false;
                }
                if t.starts_with("No config path provided") {
                    return false;
                }
                // Drop driver/dialect boilerplate
                if t.starts_with("Using '") && t.contains("driver") {
                    return false;
                }
                if t.starts_with("dialect is") {
                    return false;
                }
                // Drop "database credentials from ..." line
                if t.starts_with("database credentials from") {
                    return false;
                }
                true
            })
            .collect();

        let filtered = dedup::apply(filtered, config.dedup_min);
        truncation::apply(filtered, 80, truncation::Keep::Tail)
    }
}
