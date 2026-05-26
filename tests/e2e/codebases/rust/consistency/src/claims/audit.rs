use super::domain::ClaimRecord;
fn redact_email(email: &str) -> String {
    email
        .split_once('@')
        .map(|(_, domain)| format!("redacted@{domain}"))
        .unwrap_or_else(|| "redacted".to_string())
}
pub fn record_claim_audit_safely(claim: &ClaimRecord) -> String {
    format!(
        "claim={} patient={}",
        claim.claim_id,
        redact_email(&claim.patient_email)
    )
}
pub fn record_claim_audit_with_email(claim: &ClaimRecord) -> String {
    format!("claim={} patient={}", claim.claim_id, claim.patient_email)
}
