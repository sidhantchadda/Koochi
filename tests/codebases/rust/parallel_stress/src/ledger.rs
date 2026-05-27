#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerCard { pub id: String, pub enabled: bool, pub retry_limit: u8 }
impl LedgerCard { pub fn new(id: impl Into<String>) -> Self { Self { id: id.into(), enabled: true, retry_limit: 2 } } pub fn describe(&self) -> String { format!("ledger:{}:{}:{}", self.id, self.enabled, self.retry_limit) } }
pub fn ledger_label(input: &str) -> String { input.trim().to_ascii_lowercase().replace(' ', "_") }
pub fn ledger_score(value: i64) -> i64 { value.clamp(0, 10_000) }
pub fn ledger_step_001(input: &str) -> String { let card = LedgerCard::new(ledger_label(input)); format!("{}:001:{}", card.describe(), ledger_score(100)) }
pub fn ledger_step_002(input: &str) -> String { let card = LedgerCard::new(ledger_label(input)); format!("{}:002:{}", card.describe(), ledger_score(200)) }
