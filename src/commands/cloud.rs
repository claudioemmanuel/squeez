use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{smart_filter, truncation};

pub struct CloudHandler;

impl Handler for CloudHandler {
    fn compress(&self, cmd: &str, lines: Vec<String>, _config: &Config) -> Vec<String> {
        // az boards/repos work items return dense JSON — extract only key fields.
        if is_az_workitem_cmd(cmd) {
            if let Some(extracted) = extract_az_json(&lines) {
                return extracted;
            }
        }
        let lines = smart_filter::apply(lines);
        let filtered: Vec<String> = lines
            .into_iter()
            .filter(|l| !l.starts_with("---") && !l.trim().is_empty())
            .collect();
        truncation::apply(filtered, 100, truncation::Keep::Head)
    }
}

fn is_az_workitem_cmd(cmd: &str) -> bool {
    let c = cmd.trim();
    (c.contains("az boards") || c.contains("az repos")) && !c.contains("az repos pr list")
}

/// Parse lines as JSON and return a compact summary of key fields.
/// Keeps: id, rev, System.* fields, url (shortened). Drops: _links, relations, extensions.
fn extract_az_json(lines: &[String]) -> Option<Vec<String>> {
    let raw = lines.join("\n");
    // Quick pre-check — skip non-JSON output (table/tsv format).
    let trimmed = raw.trim_start();
    if !trimmed.starts_with('{') && !trimmed.starts_with('[') {
        return None;
    }

    // Use a simple line-based extractor rather than a JSON parser (zero-dep constraint).
    let mut out: Vec<String> = Vec::new();
    let mut in_links = false;
    let mut in_relations = false;
    let mut brace_depth: i32 = 0;

    for line in lines {
        let t = line.trim();

        // Track nesting to suppress _links / relations blocks.
        let opens = t.chars().filter(|&c| c == '{' || c == '[').count() as i32;
        let closes = t.chars().filter(|&c| c == '}' || c == ']').count() as i32;

        if t.contains("\"_links\"") || t.contains("\"relations\"") || t.contains("\"extensions\"") {
            in_links = true;
            brace_depth = 0;
        }

        if in_links || in_relations {
            brace_depth += opens - closes;
            if brace_depth <= 0 {
                in_links = false;
                in_relations = false;
            }
            continue;
        }

        // Keep System.* fields, top-level id/rev/url, and structural braces.
        let keep = t.contains("\"System.")
            || t.contains("\"id\"")
            || t.contains("\"rev\"")
            || t.contains("\"url\"")
            || t.contains("\"state\"")
            || t.contains("\"title\"")
            || t == "{" || t == "}" || t == "}," || t == "\"fields\": {";

        if keep {
            out.push(line.clone());
        }
    }

    if out.is_empty() {
        return None;
    }

    out.insert(0, "[squeez: az — kept System.* fields; dropped links, relations, extensions]".to_string());
    Some(out)
}
