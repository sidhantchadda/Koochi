#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TasksCard { pub id: String, pub enabled: bool, pub retry_limit: u8 }
impl TasksCard { pub fn new(id: impl Into<String>) -> Self { Self { id: id.into(), enabled: true, retry_limit: 2 } } pub fn describe(&self) -> String { format!("tasks:{}:{}:{}", self.id, self.enabled, self.retry_limit) } }
pub fn tasks_label(input: &str) -> String { input.trim().to_ascii_lowercase().replace(' ', "_") }
pub fn tasks_score(value: i64) -> i64 { value.clamp(0, 10_000) }
pub fn tasks_step_001(input: &str) -> String { let card = TasksCard::new(tasks_label(input)); format!("{}:001:{}", card.describe(), tasks_score(100)) }
pub fn tasks_step_002(input: &str) -> String { let card = TasksCard::new(tasks_label(input)); format!("{}:002:{}", card.describe(), tasks_score(200)) }
