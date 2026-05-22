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
fn creator_market_flags_javascript_security_issues() {
    let live = live_provider();
    let temp = tempfile::tempdir().unwrap();
    copy_fixture_codebase("js", "creator_market", temp.path());
    write_koochi_config(
        temp.path(),
        &live,
        3,
        3,
        r#"
        [[test]]
        id = "creator-sql-injection"
        severity = "critical"
        instruction = "Review this JavaScript creator marketplace backend for SQL injection caused by template literals or string concatenation in SQL queries. Return failed with concrete evidence if user-controlled values are interpolated into SQL; otherwise return passed."

        [[test]]
        id = "creator-secret-logging"
        severity = "high"
        instruction = "Review this JavaScript creator marketplace backend for secret leakage in logs, including API keys, authorization headers, and cookies. Return failed with concrete evidence if secrets are logged; otherwise return passed."

        [[test]]
        id = "creator-unsupervised-job"
        severity = "medium"
        instruction = "Review this JavaScript codebase for unsupervised repeated async work using intervals or timers without cancellation, error handling, backoff, or concurrency guard. Return failed with concrete evidence if found; otherwise return passed."
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
            "creator-sql-injection",
            "creator-secret-logging",
            "creator-unsupervised-job",
        ]),
    );
    assert_failures_have_evidence(&report);
}
