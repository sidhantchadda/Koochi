#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectorsCard { pub id: String, pub enabled: bool, pub retry_limit: u8 }
impl ConnectorsCard { pub fn new(id: impl Into<String>) -> Self { Self { id: id.into(), enabled: true, retry_limit: 2 } } pub fn describe(&self) -> String { format!("connectors:{}:{}:{}", self.id, self.enabled, self.retry_limit) } }
pub fn connectors_label(input: &str) -> String { input.trim().to_ascii_lowercase().replace(' ', "_") }
pub fn connectors_score(value: i64) -> i64 { value.clamp(0, 10_000) }
pub fn connectors_step_001(input: &str) -> String { let card = ConnectorsCard::new(connectors_label(input)); format!("{}:001:{}", card.describe(), connectors_score(100)) }
pub fn connectors_step_002(input: &str) -> String { let card = ConnectorsCard::new(connectors_label(input)); format!("{}:002:{}", card.describe(), connectors_score(200)) }
