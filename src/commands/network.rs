use crate::commands::cloud;
use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{smart_filter, truncation};

pub struct NetworkHandler;

impl Handler for NetworkHandler {
    fn compress(&self, _cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        let lines = smart_filter::apply(lines);

        if lines
            .iter()
            .any(|l| l.contains("\"errors\"") && l.contains("\"message\""))
        {
            return extract_graphql_error(&lines);
        }

        // Large JSON body (curl/wget with no --json flag) that carries tens of KB
        // on one line → condense to a keys+ids sketch. Error bodies are handled
        // above; this catches success payloads that would otherwise flood context.
        let joined = lines.join("\n");
        if config.summarize_threshold_bytes > 0
            && cloud::sniff_large_json(&joined, config.summarize_threshold_bytes)
        {
            return cloud::summarize_json_object(&joined);
        }

        // HTML response: strip tags so 50 lines of minified HTML don't flood context.
        if is_html_response(&lines) {
            let text_lines = strip_html_tags(lines);
            return truncation::apply(text_lines, 30, truncation::Keep::Head);
        }

        let filtered: Vec<String> = lines
            .into_iter()
            .filter(|l| l.starts_with("< HTTP") || l.starts_with("HTTP/") || !l.starts_with("< "))
            .collect();

        truncation::apply(filtered, 60, truncation::Keep::Head)
    }
}

fn is_html_response(lines: &[String]) -> bool {
    lines.first().map(|l| {
        let t = l.trim().to_lowercase();
        t.starts_with("<!doctype") || t.starts_with("<html")
    }).unwrap_or(false)
}

fn strip_html_tags(lines: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for line in lines {
        let text = strip_tags_from_line(&line);
        let text = text.trim().to_string();
        if !text.is_empty() {
            out.push(text);
        }
    }
    out
}

/// Remove all `<…>` spans, replace each closing `>` with a space, then
/// collapse runs of whitespace so the extracted text stays readable.
pub fn strip_tags_from_line(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    // Collapse consecutive spaces into one.
    let mut result = String::with_capacity(out.len());
    let mut prev_space = false;
    for c in out.chars() {
        if c == ' ' || c == '\t' {
            if !prev_space {
                result.push(' ');
            }
            prev_space = true;
        } else {
            result.push(c);
            prev_space = false;
        }
    }
    result
}

fn extract_graphql_error(lines: &[String]) -> Vec<String> {
    let mut result = vec!["[GraphQL Error]".to_string()];
    for line in lines {
        if let Some(i) = line.find("\"message\":") {
            let rest = &line[i + 10..].trim_start_matches('"');
            if let Some(end) = rest.find('"') {
                result.push(format!("Error: {}", &rest[..end]));
            }
        }
        if let Some(i) = line.find("\"code\":") {
            let rest = &line[i + 7..].trim_start_matches('"');
            if let Some(end) = rest.find('"') {
                result.push(format!("Code: {}", &rest[..end]));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_json_body_condensed() {
        let blob = format!(
            "{{\"id\":\"deadbeef123\",\"items\":[1,2,3],\"pad\":\"{}\"}}",
            "y".repeat(40000)
        );
        let out = NetworkHandler.compress("curl https://api.example.com/data", vec![blob], &Config::default());
        let joined = out.join("\n");
        assert!(joined.contains("condensed"), "{joined}");
        assert!(joined.contains("deadbeef123"), "{joined}");
    }

    #[test]
    fn small_json_body_untouched() {
        let out = NetworkHandler.compress(
            "curl https://api.example.com/x",
            vec!["{\"ok\":true}".to_string()],
            &Config::default(),
        );
        assert!(!out.join("\n").contains("condensed"));
    }

    #[test]
    fn graphql_error_still_extracted() {
        let line = "{\"errors\":[{\"message\":\"boom\",\"code\":\"BAD\"}]}".to_string();
        let out = NetworkHandler.compress("curl https://gql", vec![line], &Config::default());
        assert!(out.join("\n").contains("GraphQL Error"));
    }
}
