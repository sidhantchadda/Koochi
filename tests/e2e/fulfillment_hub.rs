use crate::support::ExpectedReport;
use crate::support::assert_failures_have_evidence;
use crate::support::assert_report_matches;
use crate::support::copy_fixture_codebase;
use crate::support::live_provider;
use crate::support::read_json;
use crate::support::run_koochi_with_live_provider;
use crate::support::write_koochi_config;

#[test]
#[ignore = "requires live provider API key and spends provider quota"]
fn fulfillment_hub_flags_payment_timeout_retry_issue() {
    let live = live_provider();
    let temp = tempfile::tempdir().unwrap();
    copy_fixture_codebase("rust", "fulfillment_hub", temp.path());
    write_koochi_config(
        temp.path(),
        &live,
        1,
        1,
        r#"
        [[test]]
        id = "fulfillment-payment-timeout-retry"
        severity = "high"
        instruction = "Review the repository for external payment or courier API calls that should have timeout or retry handling. Return failed with evidence if you find concrete code that lacks retry or timeout handling; otherwise return passed."
        "#,
    );
    let report_path = temp.path().join("report.json");

    let output = run_koochi_with_live_provider(temp.path(), &report_path, &live);

    assert_eq!(
        output.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = read_json(&report_path);
    assert_report_matches(
        &report,
        ExpectedReport::all_failed(&["fulfillment-payment-timeout-retry"]),
    );
    assert_failures_have_evidence(&report);
    assert!(
        report["failed"][0]["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|evidence| evidence["path"] == "src/delivery/payments.rs")
    );
}
