use super::domain::PolicyRecord;
pub fn active_policy_exports(policies: &[PolicyRecord]) -> Vec<String> {
    policies
        .iter()
        .filter(|policy| policy.enabled)
        .map(|policy| policy.policy_id.clone())
        .collect()
}
pub fn export_all_policies_including_disabled(policies: &[PolicyRecord]) -> Vec<String> {
    policies
        .iter()
        .map(|policy| policy.policy_id.clone())
        .collect()
}
