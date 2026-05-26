use crate::agents::AgentVerdict;
use crate::llm::TestStatus;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SynthesisReport {
    pub passed: Vec<AgentVerdict>,
    pub failed: Vec<AgentVerdict>,
}

impl SynthesisReport {
    pub fn has_failures(&self) -> bool {
        !self.failed.is_empty()
    }
}

pub fn synthesize_results(mut verdicts: Vec<AgentVerdict>) -> SynthesisReport {
    verdicts.sort_by(|left, right| {
        right
            .severity
            .map(crate::Severity::rank)
            .unwrap_or(0)
            .cmp(&left.severity.map(crate::Severity::rank).unwrap_or(0))
            .then_with(|| first_path(left).cmp(first_path(right)))
            .then_with(|| first_line(left).cmp(&first_line(right)))
            .then_with(|| left.test_id.cmp(&right.test_id))
    });

    let mut passed = Vec::new();
    let mut failed = Vec::new();
    for verdict in verdicts {
        match verdict.status {
            TestStatus::Passed => passed.push(verdict),
            TestStatus::Failed => failed.push(verdict),
        }
    }
    SynthesisReport { passed, failed }
}

fn first_path(verdict: &AgentVerdict) -> &str {
    verdict
        .evidence
        .first()
        .map(|evidence| evidence.path.as_str())
        .unwrap_or("")
}

fn first_line(verdict: &AgentVerdict) -> u32 {
    verdict
        .evidence
        .first()
        .map(|evidence| evidence.line)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Severity;

    #[test]
    fn separates_and_sorts_results() {
        let report = synthesize_results(vec![
            AgentVerdict {
                test_id: "low".to_string(),
                status: TestStatus::Failed,
                severity: Some(Severity::Low),
                description: "low".to_string(),
                evidence: Vec::new(),
                elapsed_ms: 10,
            },
            AgentVerdict {
                test_id: "high".to_string(),
                status: TestStatus::Failed,
                severity: Some(Severity::High),
                description: "high".to_string(),
                evidence: Vec::new(),
                elapsed_ms: 20,
            },
            AgentVerdict {
                test_id: "pass".to_string(),
                status: TestStatus::Passed,
                severity: None,
                description: "pass".to_string(),
                evidence: Vec::new(),
                elapsed_ms: 5,
            },
        ]);
        assert_eq!(report.passed.len(), 1);
        assert_eq!(report.failed[0].test_id, "high");
    }
}
