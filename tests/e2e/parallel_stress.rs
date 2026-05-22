use crate::support::E2eCase;
use crate::support::ExpectedReport;
use crate::support::Fixture;
use crate::support::run_case;

#[test]
fn live_provider_runs_fifty_parallel_agentic_tests() {
    let passed = [
        "pass-authorization-guard",
        "pass-billing-authorization",
        "pass-report-authorization",
        "pass-job-authorization",
        "pass-parameterized-sql",
        "pass-tenant-scoped-query",
        "pass-pagination-limit",
        "pass-idempotency-storage",
        "pass-redacted-logging",
        "pass-audit-redaction",
        "pass-integer-cents-money",
        "pass-timeout-retry-payment",
        "pass-discount-bounds",
        "pass-idempotency-key",
        "pass-single-flight-cache",
        "pass-tenant-cache-key",
        "pass-bounded-background-job",
        "pass-queue-retry-policy",
        "pass-path-allowlist",
        "pass-safe-file-export",
        "pass-http-auth-flow",
        "pass-list-auth-pagination",
        "pass-webhook-signature",
        "pass-external-timeout",
        "pass-config-validation",
        "pass-feature-flag",
        "pass-tenant-filter",
        "pass-referenced-helper",
        "pass-support-role-check",
        "pass-scope-check",
        "pass-audit-parameterized-sql",
        "pass-delete-tenant-scoped",
        "pass-trace-field-filter",
        "pass-metric-normalization",
        "pass-cache-ttl",
        "pass-feature-cache-key",
        "pass-retry-backoff",
        "pass-digest-enqueue-bound",
        "pass-report-name-sanitizer",
        "pass-webhook-acceptance",
    ];
    let failed = [
        "fail-secret-logging",
        "fail-sql-interpolation",
        "fail-missing-org-auth",
        "fail-cache-stampede",
        "fail-no-timeout-payment-call",
        "fail-money-as-float",
        "fail-path-traversal",
        "fail-unbounded-background-loop",
        "fail-dead-code",
        "fail-tenant-data-leak",
    ];
    let run = run_case(
        E2eCase::live_fixture_config(
            &[Fixture::Copy {
                language: "rust",
                name: "parallel_stress",
            }],
            50,
            50,
            ExpectedReport {
                passed: &passed,
                failed: &failed,
            },
        )
        .with_debug(),
    );

    let debug_log = run.debug_log.unwrap();
    assert!(
        debug_log["llm"]["turns"].as_u64().unwrap_or_default() >= 100,
        "expected at least two live LLM turns per agent on average, got debug log: {debug_log:#}"
    );
    assert_eq!(debug_log["llm"]["agent_batches"].as_u64(), Some(1));
    assert!(
        debug_log["llm"]["provider_calls"]
            .as_u64()
            .unwrap_or_default()
            >= debug_log["llm"]["turns"].as_u64().unwrap_or_default(),
        "expected provider calls to cover all LLM turns, got debug log: {debug_log:#}"
    );
    assert_eq!(
        debug_log["search"]["list_review_files_calls"].as_u64(),
        Some(50)
    );
    assert!(
        debug_log["search"]["search_text_calls"]
            .as_u64()
            .unwrap_or_default()
            >= 50,
        "expected each live stress agent to search for its marker, got debug log: {debug_log:#}"
    );
    assert!(
        debug_log["search"]["read_file_calls"]
            .as_u64()
            .unwrap_or_default()
            > 0,
        "expected read_file usage, got debug log: {debug_log:#}"
    );
    assert!(
        debug_log["search"]["get_file_context_calls"]
            .as_u64()
            .unwrap_or_default()
            > 0,
        "expected get_file_context usage, got debug log: {debug_log:#}"
    );
    assert!(
        debug_log["search"]["definition_calls"]
            .as_u64()
            .unwrap_or_default()
            > 0,
        "expected find_definitions usage, got debug log: {debug_log:#}"
    );
    assert!(
        debug_log["search"]["reference_calls"]
            .as_u64()
            .unwrap_or_default()
            > 0,
        "expected find_references usage, got debug log: {debug_log:#}"
    );
}
