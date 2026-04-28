use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{smart_filter, truncation};

pub struct DatabaseHandler;

impl Handler for DatabaseHandler {
    fn compress(&self, cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        // prisma generate emits ~5 boilerplate lines; only the ✔/error line matters.
        if cmd.contains("prisma") && cmd.contains("generate") {
            return prisma_generate_compress(lines);
        }
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

fn prisma_generate_compress(lines: Vec<String>) -> Vec<String> {
    let lines = smart_filter::apply(lines);
    // Keep lines that contain the generation result or an error.
    let result: Vec<String> = lines
        .into_iter()
        .filter(|l| {
            let t = l.to_lowercase();
            t.contains("generated prisma client")
                || t.contains("error")
                || t.contains("warning")
                || t.contains("✔")
                || t.contains("✓")
        })
        .collect();
    if result.is_empty() {
        vec!["[squeez: prisma generate — no output captured]".to_string()]
    } else {
        result
    }
}
