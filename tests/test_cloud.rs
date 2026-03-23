use squeez::commands::{cloud::CloudHandler, Handler};
use squeez::config::Config;

#[test]
fn kubectl_strips_separator_lines() {
    let lines = vec![
        "NAME                    READY   STATUS    RESTARTS   AGE".to_string(),
        "api-7d9f8b-xyz          1/1     Running   0          2d".to_string(),
        "worker-abc-123          0/1     Error     3          1h".to_string(),
    ];
    let result = CloudHandler.compress("kubectl get pods", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("Running")));
    assert!(result.iter().any(|l| l.contains("Error")));
}
