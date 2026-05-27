fn normalize_status(status: &str) -> &'static str {
    match status {
        "approved" => "approved",
        "denied" => "denied",
        "pending" => "pending",
        _ => "other",
    }
}
pub fn claim_metric_labels(status: &str) -> Vec<(&'static str, String)> {
    vec![("status", normalize_status(status).to_string())]
}
pub fn claim_metric_labels_raw(status: &str) -> Vec<(&'static str, String)> {
    vec![("status", status.to_string())]
}
