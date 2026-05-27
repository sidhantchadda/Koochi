use crate::support::ExpectedReport;
use crate::support::Fixture;
use crate::support::LiveProviderCase;
use crate::support::assert_report_matches_with_debug;
use crate::support::copy_fixture_codebase;
use crate::support::format_debug_summary;
use crate::support::koochi_bin;
use crate::support::live_provider_for_config;
use crate::support::read_json;
use crate::support::try_latest_debug_log;
use std::process::Command;

#[test]
fn live_provider_discovers_config_writes_report_and_returns_failure_exit() {
    let run = LiveProviderCase::live_fixture_config(
        &[Fixture::Copy {
            language: "rust",
            name: "config_discovery",
        }],
        ExpectedReport::all_failed(&["fail-config-discovery-live"]),
    )
    .run_with_config_name("KOOCHI.TOML");

    let stdout = String::from_utf8(run.output.stdout).unwrap();
    assert!(stdout.contains("Running 1 agentic invariants"));
    assert!(stdout.contains("0/1 passed, 1 failed"));
}

#[test]
fn live_provider_config_flag_overrides_discovery_and_returns_failure_exit() {
    let temp = tempfile::tempdir().unwrap();
    copy_fixture_codebase("rust", "config_override", temp.path());

    let live = live_provider_for_config(&temp.path().join("override.toml"));
    let report_path = temp.path().join("report.json");

    let output = Command::new(koochi_bin())
        .current_dir(temp.path())
        .env(&live.api_key_env, &live.api_key)
        .arg("--config")
        .arg(temp.path().join("override.toml"))
        .arg("--yes")
        .arg("--debug")
        .arg("--json-output")
        .arg(&report_path)
        .output()
        .unwrap();
    let debug_log = try_latest_debug_log(temp.path());
    let debug_summary = debug_log
        .as_ref()
        .map(format_debug_summary)
        .unwrap_or_else(|| "live-provider debug metrics: no debug log was written".to_string());
    println!("{debug_summary}");

    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        debug_summary
    );
    let report = read_json(&report_path);
    assert_report_matches_with_debug(
        &report,
        ExpectedReport::all_failed(&["fail-override-config-live"]),
        &debug_summary,
    );
}
