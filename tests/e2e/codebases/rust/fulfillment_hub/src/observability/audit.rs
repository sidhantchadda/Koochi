pub struct WebhookRequest {
    pub delivery_id: String,
    pub authorization: String,
    pub cookie: String,
}

pub fn record_webhook_attempt(request: &WebhookRequest) {
    tracing::info!(
        delivery_id = %request.delivery_id,
        authorization = %request.authorization,
        cookie = %request.cookie,
        "delivery webhook received"
    );
}
