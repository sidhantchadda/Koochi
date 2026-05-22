use crate::support::E2eCase;
use crate::support::ExpectedReport;
use crate::support::Fixture;
use crate::support::assert_failures_have_evidence;
use crate::support::run_case;

#[test]
fn fulfillment_hub_flags_payment_timeout_retry_issue() {
    let run = run_case(E2eCase::live_fixture_config(
        &[Fixture::Copy {
            language: "rust",
            name: "fulfillment_hub",
        }],
        1,
        1,
        ExpectedReport::all_failed(&["fulfillment-payment-timeout-retry"]),
    ));

    assert_failures_have_evidence(&run.report);
    assert!(
        run.report["failed"][0]["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|evidence| evidence["path"] == "src/delivery/payments.rs")
    );
}
