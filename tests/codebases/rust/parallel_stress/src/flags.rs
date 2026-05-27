#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlagsCard { pub id: String, pub enabled: bool, pub retry_limit: u8 }
impl FlagsCard { pub fn new(id: impl Into<String>) -> Self { Self { id: id.into(), enabled: true, retry_limit: 2 } } pub fn describe(&self) -> String { format!("flags:{}:{}:{}", self.id, self.enabled, self.retry_limit) } }
pub fn flags_label(input: &str) -> String { input.trim().to_ascii_lowercase().replace(' ', "_") }
pub fn flags_score(value: i64) -> i64 { value.clamp(0, 10_000) }
pub fn flags_step_001(input: &str) -> String { let card = FlagsCard::new(flags_label(input)); format!("{}:001:{}", card.describe(), flags_score(100)) }
pub fn flags_step_002(input: &str) -> String { let card = FlagsCard::new(flags_label(input)); format!("{}:002:{}", card.describe(), flags_score(200)) }
