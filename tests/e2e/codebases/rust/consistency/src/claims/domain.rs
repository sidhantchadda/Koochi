#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimRecord {
    pub tenant_id: String,
    pub claim_id: String,
    pub patient_email: String,
    pub amount_cents: i64,
    pub status: String,
    pub disabled_policy: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimExportRow {
    pub tenant_id: String,
    pub claim_id: String,
    pub patient_email: String,
    pub amount_cents: i64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebhookEnvelope {
    pub signature: Option<String>,
    pub payload: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentRequest {
    pub tenant_id: String,
    pub claim_id: String,
    pub amount_cents: i64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyRecord {
    pub policy_id: String,
    pub enabled: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalRequest {
    pub amount_cents: i64,
    pub approvers: Vec<String>,
}
