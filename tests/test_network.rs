use squeez::commands::{network::NetworkHandler, Handler};
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
