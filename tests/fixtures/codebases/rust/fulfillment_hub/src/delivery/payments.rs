use crate::Error;

pub struct CourierPaymentClient;
pub struct PayoutRequest {
    pub courier_id: String,
    pub amount_usd: f64,
}
pub struct PayoutReceipt;

impl CourierPaymentClient {
    pub async fn create_payout(&self, _request: PayoutRequest) -> Result<PayoutReceipt, Error> {
        Ok(PayoutReceipt)
    }
}

pub async fn release_courier_payout(
    client: &CourierPaymentClient,
    courier_id: String,
    completed_miles: f64,
) -> Result<PayoutReceipt, Error> {
    let amount_usd = completed_miles * 1.75;
    client
        .create_payout(PayoutRequest {
            courier_id,
            amount_usd,
        })
        .await
}
