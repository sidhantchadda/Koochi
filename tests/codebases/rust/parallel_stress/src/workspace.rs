#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceCard { pub id: String, pub enabled: bool, pub retry_limit: u8 }
impl WorkspaceCard { pub fn new(id: impl Into<String>) -> Self { Self { id: id.into(), enabled: true, retry_limit: 2 } } pub fn describe(&self) -> String { format!("workspace:{}:{}:{}", self.id, self.enabled, self.retry_limit) } }
pub fn workspace_label(input: &str) -> String { input.trim().to_ascii_lowercase().replace(' ', "_") }
pub fn workspace_score(value: i64) -> i64 { value.clamp(0, 10_000) }
pub fn workspace_step_001(input: &str) -> String { let card = WorkspaceCard::new(workspace_label(input)); format!("{}:001:{}", card.describe(), workspace_score(100)) }
pub fn workspace_step_002(input: &str) -> String { let card = WorkspaceCard::new(workspace_label(input)); format!("{}:002:{}", card.describe(), workspace_score(200)) }
