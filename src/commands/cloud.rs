use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{smart_filter, toon, truncation};

pub struct CloudHandler;

impl Handler for CloudHandler {
    fn compress(&self, cmd: &str, lines: Vec<String>, _config: &Config) -> Vec<String> {
        // TOON re-encoding for JSON-array outputs (gh --json, kubectl -o json,
        // aws --output json, gcloud --format=json, az --output json). Lossless
        // when uniform; falls back to the standard pipeline otherwise.
        if looks_like_json_output(cmd) {
            let joined = lines.join("\n");
            if let Some(toon_out) = toon::try_to_toon(&joined) {
                return toon_out.lines().map(String::from).collect();
            }
        }
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

/// Heuristic: does this invocation request JSON output?
fn looks_like_json_output(cmd: &str) -> bool {
    let lc = cmd.to_ascii_lowercase();
    lc.contains("--json")
        || lc.contains("-o json")
        || lc.contains("-ojson")
        || lc.contains("--output json")
        || lc.contains("--output=json")
        || lc.contains("--format json")
        || lc.contains("--format=json")
        || lc.contains("-f json")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_json_request_flags() {
        assert!(looks_like_json_output("gh pr list --json number,title"));
        assert!(looks_like_json_output("kubectl get pods -o json"));
        assert!(looks_like_json_output("kubectl get pods -ojson"));
        assert!(looks_like_json_output("aws ec2 describe-instances --output json"));
        assert!(looks_like_json_output("aws ec2 describe-instances --output=json"));
        assert!(looks_like_json_output("gcloud compute instances list --format=json"));
        assert!(!looks_like_json_output("gh pr list"));
        assert!(!looks_like_json_output("kubectl get pods -o yaml"));
    }
}
