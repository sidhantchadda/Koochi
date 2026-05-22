use crate::FilePath;
use crate::Severity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmRequest {
    pub test_id: String,
    pub model: String,
    pub instruction: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmResponse {
    pub status: TestStatus,
    pub severity: Option<Severity>,
    pub description: String,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TestStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Evidence {
    pub path: FilePath,
    pub line: u32,
    pub preview: String,
}
