//! SLO check structure and pass/fail verification with injected samples.

#[test]
fn slo_check_includes_selection_summary() {
    let result = picto_core::perf::check_default_slo();
    let json = serde_json::to_value(&result).expect("serialize slo check");
    assert!(
        json.get("selection_summary").is_some(),
        "SloCheckResult must include selection_summary"
    );
    let ss = &json["selection_summary"];
    assert_eq!(ss["target_p50_ms"], 60.0);
    assert_eq!(ss["target_p95_ms"], 120.0);
    assert_eq!(ss["target_p99_ms"], 200.0);
}

#[test]
fn slo_pass_fail_with_samples() {
    for _ in 0..20 {
        picto_core::perf::record_grid_page_slim(10.0);
        picto_core::perf::record_files_metadata_batch(8.0, 3.0, 2.0, 10, 0);
        picto_core::perf::record_sidebar_tree(12.0);
        picto_core::perf::record_selection_summary(15.0);
    }

    let result = picto_core::perf::check_default_slo();
    assert!(result.pass, "SLO should pass when all samples are fast");
    assert!(result.click_metadata.pass_p50);
    assert!(result.grid_first_page.pass_p50);
    assert!(result.sidebar_tree.pass_p50);
    assert!(result.selection_summary.pass_p50);

    for _ in 0..600 {
        picto_core::perf::record_selection_summary(500.0);
    }

    let result = picto_core::perf::check_default_slo();
    assert!(
        !result.pass,
        "SLO should fail when selection_summary is slow"
    );
    assert!(
        !result.selection_summary.pass_p50,
        "selection_summary P50 should fail at 500ms vs 60ms target"
    );
}
