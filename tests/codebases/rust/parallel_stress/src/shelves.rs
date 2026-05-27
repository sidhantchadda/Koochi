#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShelvesCard { pub id: String, pub enabled: bool, pub retry_limit: u8 }
impl ShelvesCard { pub fn new(id: impl Into<String>) -> Self { Self { id: id.into(), enabled: true, retry_limit: 2 } } pub fn describe(&self) -> String { format!("shelves:{}:{}:{}", self.id, self.enabled, self.retry_limit) } }
pub fn shelves_label(input: &str) -> String { input.trim().to_ascii_lowercase().replace(' ', "_") }
pub fn shelves_score(value: i64) -> i64 { value.clamp(0, 10_000) }
pub fn shelves_step_001(input: &str) -> String { let card = ShelvesCard::new(shelves_label(input)); format!("{}:001:{}", card.describe(), shelves_score(100)) }
pub fn shelves_step_002(input: &str) -> String { let card = ShelvesCard::new(shelves_label(input)); format!("{}:002:{}", card.describe(), shelves_score(200)) }
