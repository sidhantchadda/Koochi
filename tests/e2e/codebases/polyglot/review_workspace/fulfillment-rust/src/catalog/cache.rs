use crate::Error;
use std::collections::HashMap;

pub struct MenuSnapshot;
pub struct MenuService;

impl MenuService {
    pub async fn fetch_menu_snapshot(&self, _merchant_id: &str) -> Result<MenuSnapshot, Error> {
        Ok(MenuSnapshot)
    }
}

pub async fn menu_snapshot<'a>(
    cache: &'a mut HashMap<String, MenuSnapshot>,
    service: &MenuService,
    merchant_id: String,
) -> Result<&'a MenuSnapshot, Error> {
    if !cache.contains_key(&merchant_id) {
        let snapshot = service.fetch_menu_snapshot(&merchant_id).await?;
        cache.insert(merchant_id.clone(), snapshot);
    }

    Ok(cache.get(&merchant_id).expect("cache populated"))
}
