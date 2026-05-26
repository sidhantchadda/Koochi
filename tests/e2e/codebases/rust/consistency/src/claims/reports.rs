use std::path::PathBuf;
fn allowed_report_slug(name: &str) -> Option<&str> {
    match name {
        "claims-daily" | "claims-monthly" | "denials" => Some(name),
        _ => None,
    }
}
pub fn claim_report_path(base: &str, requested_name: &str) -> Option<PathBuf> {
    allowed_report_slug(requested_name)
        .map(|slug| PathBuf::from(base).join(format!("{slug}.csv")))
}
pub fn claim_report_path_unchecked(base: &str, requested_name: &str) -> PathBuf {
    PathBuf::from(base).join(requested_name)
}
