use std::collections::HashMap;
pub fn tenant_feature_enabled(
    flags: &HashMap<String, bool>,
    tenant_id: &str,
    feature: &str,
) -> bool {
    flags
        .get(&format!("tenant:{tenant_id}:{feature}"))
        .copied()
        .unwrap_or(false)
}
pub fn global_feature_enabled(
    flags: &HashMap<String, bool>,
    _tenant_id: &str,
    feature: &str,
) -> bool {
    flags
        .get(&format!("global:{feature}"))
        .copied()
        .unwrap_or(false)
}
