use crate::support::ExpectedReport;
use crate::support::assert_report_matches;
use crate::support::copy_fixture_codebase;
use crate::support::koochi_bin;
use crate::support::read_json;
use std::fs;
use std::process::Command;

#[test]
fn fake_provider_discovers_config_writes_report_and_returns_failure_exit() {
    let temp = tempfile::tempdir().unwrap();
    copy_fixture_codebase("rust", "smoke", temp.path());
    fs::write(
        temp.path().join("KOOCHI.TOML"),
        r#"
        provider = "fake"
        max_parallel_agents = 2

        tests = [
          "Simple pass",
          "Check whether API calls need retry handling."
        ]
        "#,
    )
    .unwrap();
    let report_path = temp.path().join("report.json");

    let output = Command::new(koochi_bin())
        .current_dir(temp.path())
        .arg("--json-output")
        .arg(&report_path)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Koochi: 2 agentic tests run, 1 passed, 1 failed"));

    let report = read_json(&report_path);
    assert_report_matches(
        &report,
        ExpectedReport {
            passed: &["test-1"],
            failed: &["test-2"],
        },
    );
}

#[test]
fn fake_provider_config_flag_overrides_discovery_and_returns_success_exit() {
    let temp = tempfile::tempdir().unwrap();
    copy_fixture_codebase("rust", "smoke", temp.path());
    fs::write(temp.path().join("koochi.toml"), "tests = [\"Check retry\"]").unwrap();
    fs::write(
        temp.path().join("override.toml"),
        r#"
        provider = "fake"
        tests = ["Simple pass"]
        "#,
    )
    .unwrap();
    let report_path = temp.path().join("report.json");

    let output = Command::new(koochi_bin())
        .current_dir(temp.path())
        .arg("--config")
        .arg(temp.path().join("override.toml"))
        .arg("--json-output")
        .arg(&report_path)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let report = read_json(&report_path);
    assert_report_matches(
        &report,
        ExpectedReport {
            passed: &["test-1"],
            failed: &[],
        },
    );
}
