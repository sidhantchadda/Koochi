use super::domain::PaymentRequest;
pub struct PaymentCommand {
    pub amount_cents: i64,
    pub idempotency_key: Option<String>,
    pub retry_budget: u8,
}
pub fn submit_claim_payment_safely(request: &PaymentRequest) -> PaymentCommand {
    PaymentCommand {
        amount_cents: request.amount_cents,
        idempotency_key: Some(format!("{}:{}:payment", request.tenant_id, request.claim_id)),
        retry_budget: 3,
    }
}
pub fn submit_claim_payment_without_idempotency(request: &PaymentRequest) -> PaymentCommand {
    PaymentCommand {
        amount_cents: request.amount_cents,
        idempotency_key: None,
        retry_budget: 0,
    }
}
