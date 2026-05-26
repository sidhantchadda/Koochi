use super::domain::{ClaimExportRow, ClaimRecord};
fn row_for_claim(claim: &ClaimRecord) -> ClaimExportRow {
    ClaimExportRow {
        tenant_id: claim.tenant_id.clone(),
        claim_id: claim.claim_id.clone(),
        patient_email: claim.patient_email.clone(),
        amount_cents: claim.amount_cents,
    }
}
fn sanitize_csv_cell(value: &str) -> String {
    let cleaned = value.replace(['\n', '\r'], " ");
    if cleaned.starts_with(['=', '+', '-', '@']) {
        format!("'{}", cleaned)
    } else {
        cleaned
    }
}
pub fn export_claims_for_tenant(claims: &[ClaimRecord], tenant_id: &str) -> Vec<ClaimExportRow> {
    claims
        .iter()
        .filter(|claim| claim.tenant_id == tenant_id)
        .map(row_for_claim)
        .collect()
}
pub fn export_claims_without_tenant_filter(
    claims: &[ClaimRecord],
    _tenant_id: &str,
) -> Vec<ClaimExportRow> {
    claims.iter().map(row_for_claim).collect()
}
pub fn export_claims_csv_safely(rows: &[ClaimExportRow]) -> String {
    rows
        .iter()
        .map(|row| {
            format!(
                "{},{},{},{}",
                sanitize_csv_cell(&row.tenant_id),
                sanitize_csv_cell(&row.claim_id),
                sanitize_csv_cell(&row.patient_email),
                row.amount_cents
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
pub fn export_claims_csv_raw(rows: &[ClaimExportRow]) -> String {
    rows
        .iter()
        .map(|row| {
            format!(
                "{},{},{},{}",
                row.tenant_id, row.claim_id, row.patient_email, row.amount_cents
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
