use crate::support::fixture_codebase;
use serde_json::Value;
use std::collections::BTreeSet;

pub fn expected_ids(prefix: &str) -> BTreeSet<String> {
    let config =
        std::fs::read_to_string(fixture_codebase("rust", "consistency").join("koochi.toml"))
            .unwrap();
    config
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("id = \"")
                .and_then(|value| value.strip_suffix('"'))
        })
        .filter(|id| id.starts_with(prefix))
        .map(ToString::to_string)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub passed: BTreeSet<String>,
    pub failed: BTreeSet<String>,
}

pub fn report_outcome(report: &Value) -> RunOutcome {
    RunOutcome {
        passed: report_ids(report, "passed"),
        failed: report_ids(report, "failed"),
    }
}

fn report_ids(report: &Value, field: &str) -> BTreeSet<String> {
    report[field]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["test_id"].as_str().unwrap().to_string())
        .collect()
}
