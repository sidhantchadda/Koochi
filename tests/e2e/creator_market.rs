use crate::support::E2eCase;
use crate::support::ExpectedReport;
use crate::support::Fixture;
use crate::support::assert_failures_have_evidence;
use crate::support::run_case;

#[test]
fn creator_market_flags_javascript_security_issues() {
    let run = run_case(E2eCase::live_fixture_config(
        &[Fixture::Copy {
            language: "js",
            name: "creator_market",
        }],
        3,
        3,
        ExpectedReport::all_failed(&[
            "creator-sql-injection",
            "creator-secret-logging",
            "creator-unsupervised-job",
        ]),
    ));

    assert_failures_have_evidence(&run.report);
}
