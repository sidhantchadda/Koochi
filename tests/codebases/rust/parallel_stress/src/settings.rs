#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsCard { pub id: String, pub enabled: bool, pub retry_limit: u8 }
impl SettingsCard { pub fn new(id: impl Into<String>) -> Self { Self { id: id.into(), enabled: true, retry_limit: 2 } } pub fn describe(&self) -> String { format!("settings:{}:{}:{}", self.id, self.enabled, self.retry_limit) } }
pub fn settings_label(input: &str) -> String { input.trim().to_ascii_lowercase().replace(' ', "_") }
pub fn settings_score(value: i64) -> i64 { value.clamp(0, 10_000) }
pub fn settings_step_001(input: &str) -> String { let card = SettingsCard::new(settings_label(input)); format!("{}:001:{}", card.describe(), settings_score(100)) }
pub fn settings_step_002(input: &str) -> String { let card = SettingsCard::new(settings_label(input)); format!("{}:002:{}", card.describe(), settings_score(200)) }
