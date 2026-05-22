pub mod agents;
pub mod cli;
pub mod config;
pub mod llm;
pub mod prompts;
pub mod scope;
pub mod search;
pub mod synthesis;

pub use agents::*;
pub use cli::*;
pub use config::*;
pub use llm::*;
pub use prompts::*;
pub use scope::*;
pub use search::*;
pub use synthesis::*;

pub type AgentId = String;
pub type FilePath = String;
pub type RepoId = String;
pub type ToolName = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub fn rank(self) -> u8 {
        match self {
            Severity::Low => 0,
            Severity::Medium => 1,
            Severity::High => 2,
            Severity::Critical => 3,
        }
    }
}
