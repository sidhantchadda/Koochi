#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportsCard { pub id: String, pub enabled: bool, pub retry_limit: u8 }
impl ReportsCard { pub fn new(id: impl Into<String>) -> Self { Self { id: id.into(), enabled: true, retry_limit: 2 } } pub fn describe(&self) -> String { format!("reports:{}:{}:{}", self.id, self.enabled, self.retry_limit) } }
pub fn reports_label(input: &str) -> String { input.trim().to_ascii_lowercase().replace(' ', "_") }
pub fn reports_score(value: i64) -> i64 { value.clamp(0, 10_000) }
pub fn reports_step_001(input: &str) -> String { let card = ReportsCard::new(reports_label(input)); format!("{}:001:{}", card.describe(), reports_score(100)) }
pub fn reports_step_002(input: &str) -> String { let card = ReportsCard::new(reports_label(input)); format!("{}:002:{}", card.describe(), reports_score(200)) }
