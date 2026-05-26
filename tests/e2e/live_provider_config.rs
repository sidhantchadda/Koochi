use crate::support::E2eCase;
use crate::support::ExpectedReport;
use crate::support::Fixture;
use crate::support::assert_report_matches;
use crate::support::copy_fixture_codebase;
use crate::support::koochi_bin;
use crate::support::live_provider;
use crate::support::read_json;
use std::fs;
use std::process::Command;

#[test]
fn live_provider_discovers_config_writes_report_and_returns_failure_exit() {
    let run = E2eCase::live_fixture_config(
        &[Fixture::Copy {
            language: "rust",
            name: "config_discovery",
        }],
        1,
        1,
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

    let live = live_provider();
    rewrite_live_header(&temp.path().join("koochi.toml"), &live);
    rewrite_live_header(&temp.path().join("override.toml"), &live);
    let report_path = temp.path().join("report.json");

    let output = Command::new(koochi_bin())
        .current_dir(temp.path())
        .env(live.api_key_env, &live.api_key)
        .arg("--config")
        .arg(temp.path().join("override.toml"))
        .arg("--json-output")
        .arg(&report_path)
        .output()
        .unwrap();

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
        ExpectedReport::all_failed(&["fail-override-config-live"]),
    );
}

fn rewrite_live_header(path: &std::path::Path, live: &crate::support::LiveProvider) {
    let config = fs::read_to_string(path).unwrap();
    let body = config
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("provider =")
                && !trimmed.starts_with("model =")
                && !trimmed.starts_with("api_key_env =")
                && !trimmed.starts_with("max_parallel_agents =")
                && !trimmed.starts_with("max_parallel_llm_requests =")
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(path, format!("{}\n{}", live.toml_header(1, 1), body)).unwrap();
}
