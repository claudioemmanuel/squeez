use squeez::commands::benchmark::{efficiency_to_json, run_efficiency_proof};

#[test]
fn efficiency_proof_returns_five_cases() {
    let results = run_efficiency_proof();
    assert_eq!(results.len(), 5, "expected exactly 5 proof cases, got {}", results.len());

    let expected_labels = [
        "sig_mode_rust_1000",
        "sig_mode_python_1000",
        "sig_mode_delta_vs_pipeline",
        "structured_vs_prose",
        "hypothesis_c6_vs_c0",
    ];
    for label in &expected_labels {
        assert!(
            results.iter().any(|r| r.label == *label),
            "missing expected label: {}", label
        );
    }
}

#[test]
fn efficiency_proof_all_floors_pass() {
    let results = run_efficiency_proof();
    let all_pass = results.iter().all(|r| r.passes);
    if !all_pass {
        for r in &results {
            if !r.passes {
                eprintln!(
                    "FAIL  feature={} label={} reduction={:.2}% floor={:.1}%",
                    r.feature, r.label, r.reduction_pct, r.floor_pct
                );
            }
        }
    }
    assert!(all_pass, "one or more efficiency proof floors did not pass — see stderr above");
}

#[test]
fn efficiency_json_envelope_is_valid() {
    let results = run_efficiency_proof();
    let json = efficiency_to_json(&results);

    assert!(
        json.contains("\"schema_version\":1"),
        "JSON missing schema_version:1\ngot: {}",
        &json[..json.len().min(300)]
    );
    assert!(
        json.contains("\"all_pass\":"),
        "JSON missing all_pass field"
    );
    assert!(json.contains("\"efficiency_proof\":"), "JSON missing efficiency_proof array");

    for label in &[
        "sig_mode_rust_1000",
        "sig_mode_python_1000",
        "sig_mode_delta_vs_pipeline",
        "structured_vs_prose",
        "hypothesis_c6_vs_c0",
    ] {
        assert!(json.contains(label), "JSON missing label: {}", label);
    }

    for feature in &["US-001", "US-003", "US-004"] {
        assert!(json.contains(feature), "JSON missing feature: {}", feature);
    }
}

#[test]
fn sig_mode_rust_savings_above_floor() {
    let results = run_efficiency_proof();
    let entry = results
        .iter()
        .find(|r| r.label == "sig_mode_rust_1000")
        .expect("sig_mode_rust_1000 result not found");

    eprintln!(
        "sig_mode_rust_1000: baseline={}tk compressed={}tk reduction={:.2}%",
        entry.baseline_tokens, entry.compressed_tokens, entry.reduction_pct
    );

    assert!(
        entry.reduction_pct > 80.0,
        "sig_mode_rust_1000 reduction {:.2}% is not above 80.0% floor",
        entry.reduction_pct
    );
}
