use crate::commands::Handler;
use crate::config::Config;
use crate::context::factsheet;
use crate::strategies::{smart_filter, toon, truncation};

pub struct CloudHandler;

impl Handler for CloudHandler {
    fn compress(&self, cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        // JSON output is routed to TOON either when the command asks for it via a
        // flag OR when the content itself sniffs as a large JSON payload. `az`
        // emits JSON by default with no flag, so flag-detection alone misses it.
        let joined = lines.join("\n");
        let sniffed_large_json =
            config.summarize_threshold_bytes > 0 && sniff_large_json(&joined, config.summarize_threshold_bytes);
        if looks_like_json_output(cmd) || sniffed_large_json {
            if let Some(toon_out) = toon::try_to_toon(&joined) {
                return toon_out.lines().map(String::from).collect();
            }
        }
        // az boards/repos work items return dense JSON — extract only key fields.
        // Runs before the generic sniff fallback so its tailored System.* extraction
        // wins for those commands.
        if is_az_workitem_cmd(cmd) {
            if let Some(extracted) = extract_az_json(&lines) {
                return extracted;
            }
        }
        // TOON declined and it wasn't an az work-item payload, but the content is a
        // large JSON object → condense to a keys+ids sketch rather than letting
        // 50 KB pass through the generic head-truncation untouched.
        if sniffed_large_json {
            return summarize_json_object(&joined);
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

/// Content sniff: the payload begins with `{`/`[` and exceeds `byte_threshold`.
/// Catches JSON emitted without a format flag (e.g. bare `az` commands).
pub(crate) fn sniff_large_json(text: &str, byte_threshold: usize) -> bool {
    if text.len() <= byte_threshold {
        return false;
    }
    matches!(text.trim_start().as_bytes().first(), Some(b'{') | Some(b'['))
}

/// Condense a deep/non-uniform JSON payload that TOON declined into a compact
/// sketch: distinct top-of-mind keys plus precision-critical ids (via factsheet).
/// Loses structure by design — the model gets the shape and the exact ids, not
/// 50 KB of nested objects.
pub(crate) fn summarize_json_object(text: &str) -> Vec<String> {
    let kb = text.len() / 1024;
    let mut out = vec![format!("[squeez: JSON ~{} KB condensed — keys + ids preserved]", kb)];
    let keys = json_keys_sample(text, 24);
    if !keys.is_empty() {
        out.push(format!("keys: {}", keys.join(", ")));
    }
    let ids = factsheet::extract(text);
    if !ids.is_empty() {
        out.push(format!("ids: {}", ids.join(", ")));
    }
    out
}

/// Collect distinct JSON object keys (`"name":`) in encounter order, capped at
/// `max`. A hand-rolled scanner — zero-dep, tolerant of minified single-line
/// JSON. Not a validating parser; good enough for a keys sketch.
fn json_keys_sample(text: &str, max: usize) -> Vec<String> {
    use std::collections::HashSet;
    let mut keys: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            // read string contents (honor \" escape)
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() {
                if bytes[j] == b'\\' {
                    j += 2;
                    continue;
                }
                if bytes[j] == b'"' {
                    break;
                }
                j += 1;
            }
            if j >= bytes.len() {
                break;
            }
            let name = &text[start..j];
            // skip whitespace after the closing quote; a ':' marks it as a key
            let mut k = j + 1;
            while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\t' || bytes[k] == b'\n' || bytes[k] == b'\r') {
                k += 1;
            }
            if k < bytes.len() && bytes[k] == b':' && !name.is_empty() && seen.insert(name.to_string()) {
                keys.push(name.to_string());
                if keys.len() >= max {
                    break;
                }
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    keys
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

    #[test]
    fn sniff_requires_json_start_and_size() {
        let big_json = format!("{{{}}}", "\"x\":1,".repeat(6000)); // >24 KB, starts '{'
        assert!(sniff_large_json(&big_json, 24576));
        // big but not JSON
        assert!(!sniff_large_json(&"a".repeat(30000), 24576));
        // JSON but small
        assert!(!sniff_large_json("{\"a\":1}", 24576));
    }

    #[test]
    fn summarize_json_preserves_keys_and_ids() {
        // deep, non-uniform object with an exact id TOON would decline
        let blob = format!(
            "{{\"id\":\"a1b2c3d4e5f6\",\"name\":\"widget\",\"nested\":{{\"count\":42,\"tag\":\"v1.2.3\"}},\"pad\":\"{}\"}}",
            "z".repeat(30000)
        );
        let out = summarize_json_object(&blob);
        let joined = out.join("\n");
        assert!(joined.contains("keys:"), "{joined}");
        assert!(joined.contains("name"), "{joined}");
        // factsheet keeps the hex id and the version
        assert!(joined.contains("a1b2c3d4e5f6"), "{joined}");
        // condensed: far smaller than the 30 KB input
        assert!(joined.len() < 1000, "len={}", joined.len());
    }

    #[test]
    fn bare_az_json_condensed_without_flag() {
        let blob = format!(
            "{{\"id\":98765432,\"fields\":{{\"System.Title\":\"t\"}},\"blob\":\"{}\"}}",
            "q".repeat(40000)
        );
        let out = CloudHandler.compress("az pipelines runs show --id 5", vec![blob], &Config::default());
        let joined = out.join("\n");
        assert!(joined.contains("condensed"), "{joined}");
        assert!(joined.len() < 2000, "len={}", joined.len());
    }

    #[test]
    fn small_cloud_json_untouched() {
        let small = vec!["{\"id\":1,\"name\":\"a\"}".to_string()];
        let out = CloudHandler.compress("az account show", small.clone(), &Config::default());
        // no condense header — passes through the normal pipeline verbatim
        assert!(!out.join("\n").contains("condensed"));
    }
}
