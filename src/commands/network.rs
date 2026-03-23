use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{smart_filter, truncation};

pub struct NetworkHandler;

impl Handler for NetworkHandler {
    fn compress(&self, _cmd: &str, lines: Vec<String>, _config: &Config) -> Vec<String> {
        let lines = smart_filter::apply(lines);

        if lines
            .iter()
            .any(|l| l.contains("\"errors\"") && l.contains("\"message\""))
        {
            return extract_graphql_error(&lines);
        }

        let filtered: Vec<String> = lines
            .into_iter()
            .filter(|l| l.starts_with("< HTTP") || l.starts_with("HTTP/") || !l.starts_with("< "))
            .collect();

        truncation::apply(filtered, 60, truncation::Keep::Head)
    }
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
