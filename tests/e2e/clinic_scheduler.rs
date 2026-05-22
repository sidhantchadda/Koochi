use crate::support::E2eCase;
use crate::support::ExpectedReport;
use crate::support::Fixture;
use crate::support::assert_failures_have_evidence;
use crate::support::run_case;

#[test]
fn clinic_scheduler_flags_python_privacy_and_path_issues() {
    let run = run_case(E2eCase::live_fixture_config(
        &[Fixture::Copy {
            language: "python",
            name: "clinic_scheduler",
        }],
        3,
        3,
        ExpectedReport::all_failed(&[
            "clinic-missing-authorization",
            "clinic-secret-logging",
            "clinic-path-traversal",
        ]),
    ));

    assert_failures_have_evidence(&run.report);
}
