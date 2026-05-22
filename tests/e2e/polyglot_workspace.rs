use crate::support::ExpectedReport;
use crate::support::assert_failures_have_evidence;
use crate::support::assert_report_matches;
use crate::support::copy_fixture_codebase_under;
use crate::support::live_provider;
use crate::support::read_json;
use crate::support::run_koochi_with_live_provider;
use crate::support::write_koochi_config;

#[test]
#[ignore = "requires live provider API key and spends provider quota"]
fn polyglot_workspace_runs_parallel_review_agents() {
    let live = live_provider();
    let temp = tempfile::tempdir().unwrap();
    copy_fixture_codebase_under("rust", "fulfillment_hub", temp.path(), "fulfillment-rust");
    copy_fixture_codebase_under("js", "creator_market", temp.path(), "creator-market-js");
    copy_fixture_codebase_under(
        "python",
        "clinic_scheduler",
        temp.path(),
        "clinic-scheduler-python",
    );
    write_koochi_config(
        temp.path(),
        &live,
        9,
        4,
        r#"
        [[test]]
        id = "sql-injection"
        severity = "critical"
        instruction = "Review the fulfillment, creator marketplace, and clinic scheduler code for SQL injection caused by string interpolation, template literals, format strings, or concatenation in SQL queries. Return failed with concrete evidence if user-controlled values are interpolated into SQL; otherwise return passed."

        [[test]]
        id = "secret-logging"
        severity = "high"
        instruction = "Review the repository for secret leakage in logs, including tokens, API keys, authorization headers, cookies, and passwords. Return failed with concrete evidence if secrets are logged; otherwise return passed."

        [[test]]
        id = "cache-stampede"
        severity = "medium"
        instruction = "Review the repository for cache stampede risk where an expensive fetch happens on cache miss without single-flight, request coalescing, locking, or another in-flight guard. Return failed with concrete evidence if you find this pattern; otherwise return passed."

        [[test]]
        id = "missing-authorization"
        severity = "critical"
        instruction = "Review request handlers for missing authorization on account_id, org_id, project_id, user_id, or similar scoped identifiers. Return failed with concrete evidence if handlers trust scoped identifiers without checking ownership or permission; otherwise return passed."

        [[test]]
        id = "payment-timeout-retry"
        severity = "high"
        instruction = "Review payment, courier, pricing, and insurance integrations for external calls that lack timeout or retry handling. Return failed with concrete evidence if an external call is awaited without timeout or retry handling; otherwise return passed."

        [[test]]
        id = "money-as-float"
        severity = "medium"
        instruction = "Review money or payout calculations for floating point currency representations such as f64, float, Number, or decimal-free arithmetic. Return failed with concrete evidence if money is represented or calculated as a float; otherwise return passed."

        [[test]]
        id = "path-traversal"
        severity = "high"
        instruction = "Review file/report export code for path traversal risk where user-controlled names or paths are joined into filesystem reads without validation. Return failed with concrete evidence if this appears; otherwise return passed."

        [[test]]
        id = "unsupervised-background-job"
        severity = "medium"
        instruction = "Review background job or interval code for unsupervised repeated async work without cancellation, backoff, error handling, or concurrency guard. Return failed with concrete evidence if found; otherwise return passed."

        [[test]]
        id = "dead-code"
        severity = "low"
        instruction = "Review the repository for likely dead code: functions, helpers, experiments, or modules that are defined but have no apparent references or callers in the provided context. Return failed with concrete evidence if you find likely dead code; otherwise return passed."
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
    );
    assert_failures_have_evidence(&report);

    for item in report["failed"].as_array().unwrap() {
        let path = item["evidence"][0]["path"].as_str().unwrap();
        assert!(
            path.starts_with("fulfillment-rust/")
                || path.starts_with("creator-market-js/")
                || path.starts_with("clinic-scheduler-python/"),
            "unexpected evidence path: {path}"
        );
    }
}
