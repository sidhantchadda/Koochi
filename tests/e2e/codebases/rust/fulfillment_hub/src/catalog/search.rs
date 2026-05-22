use crate::Error;

pub struct WarehouseDb;
pub struct InventoryRow;

impl WarehouseDb {
    pub async fn query(&self, _sql: String) -> Result<Vec<InventoryRow>, Error> {
        Ok(Vec::new())
    }
}

pub async fn search_inventory(
    db: &WarehouseDb,
    merchant_id: &str,
    sku_prefix: &str,
) -> Result<Vec<InventoryRow>, Error> {
    let sql = format!(
        "select * from inventory where merchant_id = '{}' and sku like '{}%'",
        merchant_id, sku_prefix
    );
    db.query(sql).await
}
