pub fn claim_job_dedupe_key(tenant_id: &str, claim_id: &str, job: &str) -> String {
    format!("tenant:{tenant_id}:claim:{claim_id}:job:{job}")
}
pub fn claim_job_dedupe_key_global(_tenant_id: &str, claim_id: &str, job: &str) -> String {
    format!("claim:{claim_id}:job:{job}")
}
