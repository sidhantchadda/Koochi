use crate::support::ExpectedReport;
use crate::support::Fixture;
use crate::support::LiveProviderCase;
use crate::support::assert_failures_have_evidence;
use crate::support::run_case;

#[test]
fn polyglot_workspace_runs_parallel_review_agents() {
    let run = run_case(LiveProviderCase::live_fixture_config(
        &[Fixture::Copy {
            language: "polyglot",
            name: "review_workspace",
        }],
        ExpectedReport::all_failed(&[
            "sql-injection",
            "secret-logging",
            "cache-stampede",
            "missing-authorization",
            "payment-timeout-retry",
            "money-as-float",
            "path-traversal",
            "unsupervised-background-job",
            "dead-code",
        ]),
    ));

    assert_failures_have_evidence(&run.report);
    for item in run.report["failed"].as_array().unwrap() {
        let path = item["evidence"][0]["path"].as_str().unwrap();
        assert!(
            path.starts_with("fulfillment-rust/")
                || path.starts_with("creator-market-js/")
                || path.starts_with("clinic-scheduler-python/"),
            "unexpected evidence path: {path}"
        );
    }
}
