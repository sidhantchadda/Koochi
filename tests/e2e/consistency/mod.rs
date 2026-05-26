mod fixture;
mod report;

use crate::support::assert_failures_have_evidence;
use crate::support::koochi_bin;
use crate::support::live_provider;
use crate::support::read_json;
use fixture::create_claims_review_repo;
use report::expected_ids;
use report::report_outcome;
use std::fs;
use std::process::Command;

#[test]
fn live_provider_runs_thirty_claim_invariants_consistently() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let reports = temp.path().join("reports");
    fs::create_dir_all(&reports).unwrap();
    create_claims_review_repo(&repo);

    let live = live_provider();
    let expected_passed = expected_ids("pass");
    let expected_failed = expected_ids("fail");
    let mut outcomes = Vec::new();

    for run_index in 1..=5 {
        let report_path = reports.join(format!("run-{run_index}.json"));
        let output = Command::new(koochi_bin())
            .current_dir(&repo)
            .arg("--json-output")
            .arg(&report_path)
            .env(live.api_key_env, &live.api_key)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(1),
            "run {run_index} stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let report = read_json(&report_path);
        assert_failures_have_evidence(&report);
        let outcome = report_outcome(&report);
        assert_eq!(
            outcome.passed, expected_passed,
            "run {run_index} had unexpected passing invariants: {report:#}"
        );
        assert_eq!(
            outcome.failed, expected_failed,
            "run {run_index} had unexpected failing invariants: {report:#}"
        );
        outcomes.push(outcome);
    }

    for outcome in &outcomes[1..] {
        assert_eq!(
            outcome, &outcomes[0],
            "expected all five runs to produce identical pass/fail invariant sets"
        );
    }
}
