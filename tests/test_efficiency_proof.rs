use squeez::commands::benchmark::{
    efficiency_to_json, run_efficiency_proof, EfficiencyResult, CACHE_READ_RATE, CACHE_WRITE_RATE,
};

#[test]
fn efficiency_proof_returns_six_cases() {
    let results = run_efficiency_proof();
    assert_eq!(results.len(), 6, "expected exactly 6 proof cases, got {}", results.len());

    let expected_labels = [
        "sig_mode_rust_1000",
        "sig_mode_python_1000",
        "sig_mode_delta_vs_pipeline",
        "structured_vs_prose",
        "hypothesis_c6_vs_c0",
        "header_overhead_50call_session",
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
        json.contains("\"schema_version\":2"),
        "JSON missing schema_version:2\ngot: {}",
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
        "header_overhead_50call_session",
    ] {
        assert!(json.contains(label), "JSON missing label: {}", label);
    }

    for feature in &["US-001", "US-003", "US-004", "E1"] {
        assert!(json.contains(feature), "JSON missing feature: {}", feature);
    }

    for field in &[
        "\"cold_baseline_eff\":",
        "\"cold_compressed_eff\":",
        "\"warm_baseline_eff\":",
        "\"warm_compressed_eff\":",
        "\"net_saving_cold\":",
        "\"net_saving_warm\":",
    ] {
        assert!(json.contains(field), "JSON missing cache-aware field: {}", field);
    }
}

#[test]
fn cache_aware_effective_costs_are_consistent() {
    let results = run_efficiency_proof();
    let eps = 1e-9;
    for r in &results {
        assert!(
            (r.cold_baseline_eff - r.baseline_tokens as f64 * CACHE_WRITE_RATE).abs() < eps,
            "{}: cold_baseline_eff {} != baseline {} × {}",
            r.label, r.cold_baseline_eff, r.baseline_tokens, CACHE_WRITE_RATE
        );
        assert!(
            (r.cold_compressed_eff - r.compressed_tokens as f64 * CACHE_WRITE_RATE).abs() < eps,
            "{}: cold_compressed_eff {} != compressed {} × {}",
            r.label, r.cold_compressed_eff, r.compressed_tokens, CACHE_WRITE_RATE
        );
        assert!(
            (r.warm_baseline_eff - r.baseline_tokens as f64 * CACHE_READ_RATE).abs() < eps,
            "{}: warm_baseline_eff {} != baseline {} × {}",
            r.label, r.warm_baseline_eff, r.baseline_tokens, CACHE_READ_RATE
        );
        assert!(
            (r.warm_compressed_eff - r.compressed_tokens as f64 * CACHE_READ_RATE).abs() < eps,
            "{}: warm_compressed_eff {} != compressed {} × {}",
            r.label, r.warm_compressed_eff, r.compressed_tokens, CACHE_READ_RATE
        );
        // compressed ≤ baseline ⇒ saving ≥ 0 in both cache states
        if r.compressed_tokens <= r.baseline_tokens {
            assert!(r.cold_baseline_eff - r.cold_compressed_eff >= 0.0, "{}: negative cold saving", r.label);
            assert!(r.warm_baseline_eff - r.warm_compressed_eff >= 0.0, "{}: negative warm saving", r.label);
        }
    }
}

#[test]
fn cache_aware_json_represents_negative_savings() {
    // Compressed > baseline (compression made it bigger): savings must go
    // negative — not be floored — and survive JSON serialization.
    let r = EfficiencyResult::new("negative_case", "US-001", 100, 200, -100.0, 10.0);
    assert!(!r.passes);
    assert!(r.cold_baseline_eff - r.cold_compressed_eff < 0.0);
    assert!(r.warm_baseline_eff - r.warm_compressed_eff < 0.0);

    let json = efficiency_to_json(&[r]);
    assert!(
        json.contains("\"net_saving_cold\":-125.00"),
        "JSON missing negative cold saving: {}", json
    );
    assert!(
        json.contains("\"net_saving_warm\":-10.00"),
        "JSON missing negative warm saving: {}", json
    );
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
