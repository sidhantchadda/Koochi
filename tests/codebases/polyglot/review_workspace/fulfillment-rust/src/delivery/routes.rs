use crate::Error;

pub struct DeliveryRequest {
    pub account_id: String,
    pub delivery_id: String,
}

pub struct DeliveryRecord;
pub struct DeliveryStore;

impl DeliveryStore {
    pub async fn fetch_delivery(
        &self,
        _account_id: &str,
        _delivery_id: &str,
    ) -> Result<DeliveryRecord, Error> {
        Ok(DeliveryRecord)
    }
}

pub async fn get_delivery(
    request: DeliveryRequest,
    store: &DeliveryStore,
) -> Result<DeliveryRecord, Error> {
    store
        .fetch_delivery(&request.account_id, &request.delivery_id)
        .await
}
