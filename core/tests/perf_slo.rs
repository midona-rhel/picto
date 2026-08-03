//! SLO checks cover only metrics with a live recording site.

#[test]
fn sidebar_slo_uses_recorded_samples() {
    for _ in 0..20 {
        picto_core::perf::record_sidebar_tree(12.0);
    }

    let result = picto_core::perf::check_default_slo();
    assert!(result.pass);
    assert!(result.sidebar_tree.available);
    assert!(result.sidebar_tree.pass_p50);
    assert!(result.sidebar_tree.pass_p95);
    assert!(result.sidebar_tree.pass_p99);
    assert!(result.missing_metrics.is_empty());
}
