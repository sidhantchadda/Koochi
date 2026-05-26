use super::domain::ApprovalRequest;
const CLAIM_APPROVAL_LIMIT_CENTS: i64 = 250_000;
pub fn approve_high_value_claim(request: &ApprovalRequest) -> bool {
    request.approvers.len() >= 2 && request.approvers[0] != request.approvers[1]
}
pub fn approve_high_value_claim_with_one_approver(request: &ApprovalRequest) -> bool {
    !request.approvers.is_empty()
}
pub fn approve_claim_with_amount_limit(request: &ApprovalRequest) -> bool {
    request.amount_cents <= CLAIM_APPROVAL_LIMIT_CENTS && approve_high_value_claim(request)
}
pub fn approve_claim_without_amount_limit(request: &ApprovalRequest) -> bool {
    !request.approvers.is_empty()
}
