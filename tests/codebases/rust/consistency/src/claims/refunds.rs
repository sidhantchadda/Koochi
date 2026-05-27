use super::domain::PaymentRequest;
pub struct RefundCommand {
    pub claim_id: String,
    pub amount_cents: i64,
    pub reason: Option<String>,
}
pub fn issue_claim_refund_safely(request: &PaymentRequest, reason: &str) -> Option<RefundCommand> {
    if reason.trim().is_empty() {
        return None;
    }
    Some(RefundCommand {
        claim_id: request.claim_id.clone(),
        amount_cents: request.amount_cents,
        reason: Some(reason.trim().to_string()),
    })
}
pub fn issue_claim_refund_without_reason(request: &PaymentRequest) -> RefundCommand {
    RefundCommand {
        claim_id: request.claim_id.clone(),
        amount_cents: request.amount_cents,
        reason: None,
    }
}
