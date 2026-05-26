use crate::Severity;
use crate::llm::Evidence;
use crate::llm::TestStatus;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AgentVerdict {
    pub test_id: String,
    pub status: TestStatus,
    pub severity: Option<Severity>,
    pub description: String,
    pub evidence: Vec<Evidence>,
    pub elapsed_ms: u128,
}
