use squeez::commands::network::{strip_tags_from_line, NetworkHandler};
use squeez::commands::Handler;
use squeez::config::Config;

#[test]
fn curl_keeps_status_and_body() {
    let lines = vec![
        "< HTTP/1.1 200 OK".to_string(),
        "< Content-Type: application/json".to_string(),
        "< X-Request-Id: abc123".to_string(),
        "{\"status\": \"ok\"}".to_string(),
    ];
    let result = NetworkHandler.compress("curl https://example.com", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("200 OK")));
    assert!(result.iter().any(|l| l.contains("status")));
    assert!(!result.iter().any(|l| l.contains("X-Request-Id")));
}

#[test]
fn graphql_error_extracts_message() {
    let lines = vec![
        "{\"errors\":[{\"message\":\"Service not found\",\"extensions\":{\"code\":\"INTERNAL_SERVER_ERROR\"}}],\"data\":null}".to_string(),
    ];
    let result =
        NetworkHandler.compress("curl http://localhost/graphql", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("Service not found")));
    assert!(result.iter().any(|l| l.contains("INTERNAL_SERVER_ERROR")));
}

#[test]
fn html_response_strips_tags_preserves_text() {
    let lines = vec![
        "<!DOCTYPE html><html lang=\"en\"><head><title>My App</title></head><body>".to_string(),
        "<div class=\"container\"><h1>Hello World</h1><p>Some content here.</p></div>".to_string(),
        "<div class=\"footer\"><p>Footer text</p></div>".to_string(),
        "</body></html>".to_string(),
    ];
    let result = NetworkHandler.compress("curl http://localhost:3000", lines, &Config::default());
    // No angle brackets should remain
    assert!(
        !result.iter().any(|l| l.contains('<') || l.contains('>')),
        "HTML tags should be stripped: {:?}",
        result
    );
    // Meaningful text preserved
    assert!(result.iter().any(|l| l.contains("Hello World")));
    assert!(result.iter().any(|l| l.contains("Footer text")));
}

#[test]
fn html_response_truncated_to_30_text_lines() {
    let mut lines = vec!["<!DOCTYPE html><html>".to_string()];
    for i in 0..100 {
        lines.push(format!(
            "<div class=\"s-{}\"><p>Section {} content</p></div>",
            i, i
        ));
    }
    lines.push("</html>".to_string());
    let result = NetworkHandler.compress("curl http://localhost:3000", lines, &Config::default());
    // 30 text lines + 1 truncation notice
    assert!(result.len() <= 32, "expected ≤32 lines, got {}", result.len());
}

#[test]
fn html_doctype_lowercase_detected() {
    let lines = vec![
        "<!doctype html><html><body><p>lowercase doctype</p></body></html>".to_string(),
    ];
    let result = NetworkHandler.compress("curl http://example.com", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("lowercase doctype")));
    assert!(!result.iter().any(|l| l.contains('<')));
}

#[test]
fn non_html_json_response_unchanged() {
    let lines = vec![
        "< HTTP/1.1 200 OK".to_string(),
        "{\"status\": \"ok\", \"data\": 42}".to_string(),
    ];
    let result = NetworkHandler.compress("curl https://api.example.com", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("200 OK") || l.contains("status")));
    // JSON braces must survive (no HTML stripping applied)
    assert!(result.iter().any(|l| l.contains('{')));
}

#[test]
fn strip_tags_from_line_removes_attributes() {
    let input = "<div class=\"foo\" data-id=\"42\"><span>text content</span></div>";
    let result = strip_tags_from_line(input);
    assert!(!result.contains('<'));
    assert!(!result.contains('>'));
    assert!(result.contains("text content"));
}
