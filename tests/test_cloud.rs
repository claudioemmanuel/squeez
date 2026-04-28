use squeez::commands::{cloud::CloudHandler, Handler};
use squeez::config::Config;

#[test]
fn az_workitem_extracts_system_fields_and_drops_links() {
    let lines = vec![
        "{".to_string(),
        "  \"id\": 33479,".to_string(),
        "  \"rev\": 5,".to_string(),
        "  \"fields\": {".to_string(),
        "    \"System.Title\": \"[GOOGLE TRENDS] Webhook integration\",".to_string(),
        "    \"System.State\": \"Active\",".to_string(),
        "    \"System.Tags\": \"SALA DIGITAL\",".to_string(),
        "    \"Custom.SomeField\": \"ignored\",".to_string(),
        "    \"Microsoft.VSTS.Common.Priority\": 2".to_string(),
        "  },".to_string(),
        "  \"_links\": { \"self\": { \"href\": \"https://dev.azure.com/...\" } },".to_string(),
        "  \"relations\": [ { \"rel\": \"System.LinkTypes.Hierarchy-Reverse\" } ],".to_string(),
        "  \"url\": \"https://dev.azure.com/vibrateam/...\"".to_string(),
        "}".to_string(),
    ];
    let result = CloudHandler.compress("az boards work-item show --id 33479", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("System.Title")));
    assert!(result.iter().any(|l| l.contains("System.State")));
    assert!(result.iter().any(|l| l.contains("\"id\"")));
    assert!(!result.iter().any(|l| l.contains("_links")));
    assert!(!result.iter().any(|l| l.contains("Hierarchy-Reverse")));
    assert!(!result.iter().any(|l| l.contains("Custom.SomeField")));
    assert!(result[0].contains("[squeez: az"));
}

#[test]
fn az_non_json_output_falls_through_to_generic() {
    let lines = vec![
        "ID    Title                State".to_string(),
        "33479 [GOOGLE TRENDS]...  Active".to_string(),
    ];
    let result = CloudHandler.compress("az boards work-item list", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("Active")));
}

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
