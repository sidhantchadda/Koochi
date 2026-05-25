use crate::support::E2eCase;
use crate::support::ExpectedReport;
use crate::support::Fixture;
use crate::support::fixture_codebase;
use crate::support::run_case;

#[test]
fn live_provider_runs_one_hundred_twenty_eight_parallel_agentic_tests() {
    let (passed, failed) = parallel_stress_expected_ids();
    let passed_refs = passed.iter().map(String::as_str).collect::<Vec<_>>();
    let failed_refs = failed.iter().map(String::as_str).collect::<Vec<_>>();
    assert_eq!(passed_refs.len(), 118);
    assert_eq!(failed_refs.len(), 10);
    let run = run_case(
        E2eCase::live_fixture_config(
            &[Fixture::Copy {
                language: "rust",
                name: "parallel_stress",
            }],
            128,
            64,
            ExpectedReport {
                passed: &passed_refs,
                failed: &failed_refs,
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
        Some(128)
    );
    assert!(
        debug_log["search"]["search_text_calls"]
            .as_u64()
            .unwrap_or_default()
            >= 120,
        "expected most live stress agents to search for markers, got debug log: {debug_log:#}"
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

fn parallel_stress_expected_ids() -> (Vec<String>, Vec<String>) {
    let config =
        std::fs::read_to_string(fixture_codebase("rust", "parallel_stress").join("koochi.toml"))
            .unwrap();
    let mut passed = Vec::new();
    let mut failed = Vec::new();
    for line in config.lines() {
        let trimmed = line.trim();
        let Some(id) = trimmed
            .strip_prefix("id = \"")
            .and_then(|value| value.strip_suffix('"'))
        else {
            continue;
        };
        if id.starts_with("pass-") {
            passed.push(id.to_string());
        } else if id.starts_with("fail-") {
            failed.push(id.to_string());
        }
    }
    (passed, failed)
}
