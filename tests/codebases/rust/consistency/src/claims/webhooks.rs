use super::domain::WebhookEnvelope;
fn verify_signature(envelope: &WebhookEnvelope, secret: &str) -> bool {
    envelope.signature.as_deref() == Some(secret) && !envelope.payload.is_empty()
}
pub fn accept_signed_claim_webhook(envelope: &WebhookEnvelope, secret: &str) -> bool {
    verify_signature(envelope, secret)
}
pub fn accept_unsigned_claim_webhook(envelope: &WebhookEnvelope, _secret: &str) -> bool {
    !envelope.payload.is_empty()
}
