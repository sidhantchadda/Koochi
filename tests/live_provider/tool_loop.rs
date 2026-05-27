use crate::support::ExpectedReport;
use crate::support::Fixture;
use crate::support::LiveProviderCase;
use crate::support::run_case;

#[test]
fn live_provider_uses_multiple_llm_requests_for_tool_observations() {
    let run = run_case(
        LiveProviderCase::live_fixture_config(
            &[Fixture::Copy {
                language: "rust",
                name: "tool_loop",
            }],
            ExpectedReport {
                passed: &[],
                failed: &["fail-live-multi-turn"],
            },
        )
        .with_debug(),
    );

    let debug_log = run.debug_log.unwrap();
    assert!(
        debug_log["llm"]["turns"].as_u64().unwrap_or_default() >= 3,
        "expected at least 3 LLM turns, got debug log: {debug_log:#}"
    );
    assert!(
        debug_log["llm"]["provider_calls"]
            .as_u64()
            .unwrap_or_default()
            >= 3,
        "expected at least 3 provider calls, got debug log: {debug_log:#}"
    );
    assert!(
        debug_log["search"]["search_text_calls"]
            .as_u64()
            .unwrap_or_default()
            >= 1,
        "expected search_text call, got debug log: {debug_log:#}"
    );
    assert!(
        debug_log["search"]["read_file_calls"]
            .as_u64()
            .unwrap_or_default()
            >= 1,
        "expected read_file call, got debug log: {debug_log:#}"
    );
}
