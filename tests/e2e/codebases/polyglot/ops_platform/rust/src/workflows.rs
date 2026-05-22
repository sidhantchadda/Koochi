pub struct PaymentAttempt {
    pub account_id: String,
    pub amount_cents: i64,
    pub idempotency_key: String,
}

pub async fn charge_partner_gateway(attempt: PaymentAttempt) -> Result<(), String> {
    let endpoint = format!(
        "https://payments.example.test/accounts/{}/charges",
        attempt.account_id
    );
    let body = format!(
        "{{\"amount\":{},\"key\":\"{}\"}}",
        attempt.amount_cents, attempt.idempotency_key
    );
    send_without_timeout(endpoint, body).await
}

async fn send_without_timeout(endpoint: String, body: String) -> Result<(), String> {
    let _ = (endpoint, body);
    Ok(())
}

pub fn export_account_report(account_id: &str, report_name: &str) -> String {
    format!("/srv/reports/{account_id}/{report_name}.csv")
}

pub fn reconcile_cache_key(org_id: &str, project_id: &str) -> String {
    format!("org:{org_id}:project:{project_id}:reconcile")
}
