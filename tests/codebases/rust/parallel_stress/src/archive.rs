#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveCard { pub id: String, pub enabled: bool, pub retry_limit: u8 }
impl ArchiveCard { pub fn new(id: impl Into<String>) -> Self { Self { id: id.into(), enabled: true, retry_limit: 2 } } pub fn describe(&self) -> String { format!("archive:{}:{}:{}", self.id, self.enabled, self.retry_limit) } }
pub fn archive_label(input: &str) -> String { input.trim().to_ascii_lowercase().replace(' ', "_") }
pub fn archive_score(value: i64) -> i64 { value.clamp(0, 10_000) }
pub fn archive_step_001(input: &str) -> String { let card = ArchiveCard::new(archive_label(input)); format!("{}:001:{}", card.describe(), archive_score(100)) }
pub fn archive_step_002(input: &str) -> String { let card = ArchiveCard::new(archive_label(input)); format!("{}:002:{}", card.describe(), archive_score(200)) }
