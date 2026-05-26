pub fn claim_cache_key(tenant_id: &str, claim_id: &str) -> String {
    format!("tenant:{tenant_id}:claim:{claim_id}")
}
pub fn claim_cache_key_without_tenant(_tenant_id: &str, claim_id: &str) -> String {
    format!("claim:{claim_id}")
}
