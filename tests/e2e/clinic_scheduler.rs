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
fn clinic_scheduler_flags_python_privacy_and_path_issues() {
    let live = live_provider();
    let temp = tempfile::tempdir().unwrap();
    copy_fixture_codebase("python", "clinic_scheduler", temp.path());
    write_koochi_config(
        temp.path(),
        &live,
        3,
        3,
        r#"
        [[test]]
        id = "clinic-missing-authorization"
        severity = "critical"
        instruction = "Review this Python clinic scheduler for missing authorization on org_id, patient_id, member_id, or similar healthcare-scoped identifiers. Return failed with concrete evidence if handlers trust scoped identifiers without checking ownership or permission; otherwise return passed."

        [[test]]
        id = "clinic-secret-logging"
        severity = "high"
        instruction = "Review this Python clinic scheduler for secret or credential leakage in logs, including passwords, authorization headers, and cookies. Return failed with concrete evidence if secrets are logged; otherwise return passed."

        [[test]]
        id = "clinic-path-traversal"
        severity = "high"
        instruction = "Review this Python clinic scheduler for path traversal risk where user-controlled report names or paths are joined into filesystem reads without validation. Return failed with concrete evidence if found; otherwise return passed."
        "#,
    );
    let report_path = temp.path().join("report.json");

    let output = run_koochi_with_live_provider(temp.path(), &report_path, &live);

    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report = read_json(&report_path);
    assert_report_matches(
        &report,
        ExpectedReport::all_failed(&[
            "clinic-missing-authorization",
            "clinic-secret-logging",
            "clinic-path-traversal",
        ]),
    );
    assert_failures_have_evidence(&report);
}
