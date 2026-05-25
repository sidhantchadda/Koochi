use crate::support::E2eCase;
use crate::support::ExpectedReport;
use crate::support::Fixture;
use crate::support::run_case;

#[test]
fn live_provider_runs_fifty_parallel_agentic_tests() {
    let passed = [
        "pass-analytics-rule-001",
        "pass-analytics-rule-002",
        "pass-analytics-rule-003",
        "pass-analytics-rule-004",
        "pass-analytics-rule-005",
        "pass-analytics-rule-006",
        "pass-analytics-rule-007",
        "pass-analytics-rule-008",
        "pass-analytics-rule-009",
        "pass-analytics-rule-010",
        "pass-analytics-rule-011",
        "pass-analytics-rule-012",
        "pass-analytics-rule-013",
        "pass-analytics-rule-014",
        "pass-compliance-policy-001",
        "pass-compliance-policy-002",
        "pass-compliance-policy-003",
        "pass-compliance-policy-004",
        "pass-compliance-policy-005",
        "pass-compliance-policy-006",
        "pass-compliance-policy-007",
        "pass-compliance-policy-008",
        "pass-compliance-policy-009",
        "pass-compliance-policy-010",
        "pass-compliance-policy-011",
        "pass-compliance-policy-012",
        "pass-compliance-policy-013",
        "pass-workflow-route-001",
        "pass-workflow-route-002",
        "pass-workflow-route-003",
        "pass-workflow-route-004",
        "pass-workflow-route-005",
        "pass-workflow-route-006",
        "pass-workflow-route-007",
        "pass-workflow-route-008",
        "pass-workflow-route-009",
        "pass-workflow-route-010",
        "pass-workflow-route-011",
        "pass-workflow-route-012",
        "pass-workflow-route-013",
    ];
    let failed = [
        "fail-analytics-tenant-bypass",
        "fail-analytics-unbounded-score",
        "fail-analytics-disabled-signal",
        "fail-analytics-global-cache-key",
        "fail-compliance-tenant-leak",
        "fail-compliance-unsanitized-csv",
        "fail-compliance-disabled-policy",
        "fail-workflow-missing-tenant",
        "fail-workflow-single-approver",
        "fail-workflow-unbounded-amount",
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
        debug_log["llm"]["turns"].as_u64().unwrap_or_default() >= 50,
        "expected at least one live LLM turn per agent, got debug log: {debug_log:#}"
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
            + debug_log["search"]["get_file_context_calls"]
                .as_u64()
                .unwrap_or_default()
            + debug_log["search"]["get_hunk_context_calls"]
                .as_u64()
                .unwrap_or_default()
            > 0,
        "expected file context usage, got debug log: {debug_log:#}"
    );
}
