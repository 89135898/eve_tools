use crate::{
    EsiError, EsiMarketHistoryDay, EsiMarketOrder, EsiOrderType, EsiTypeInfo,
    ResolvedInventoryType, UniverseIdsResponse,
};

#[derive(Clone, Debug)]
pub struct EsiClient {
    base_url: String,
    http: reqwest::Client,
}

impl EsiClient {
    pub fn tranquility() -> Self {
        Self::new("https://esi.evetech.net")
    }

    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn resolve_inventory_type(
        &self,
        query: &str,
    ) -> Result<ResolvedInventoryType, EsiError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Err(EsiError::ItemNotFound);
        }

        if let Ok(type_id) = trimmed.parse::<i32>() {
            let info = self.type_info(type_id).await?;
            return Ok(ResolvedInventoryType {
                type_id: info.type_id,
                name: info.name,
            });
        }

        let ids = self.universe_ids(trimmed).await?;
        let entry = ids
            .inventory_types
            .unwrap_or_default()
            .into_iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(trimmed))
            .ok_or(EsiError::ItemNotFound)?;

        Ok(ResolvedInventoryType {
            type_id: entry.id,
            name: entry.name,
        })
    }

    pub async fn universe_ids(&self, _name: &str) -> Result<UniverseIdsResponse, EsiError> {
        Err(EsiError::ItemNotFound)
    }

    pub async fn type_info(&self, _type_id: i32) -> Result<EsiTypeInfo, EsiError> {
        Err(EsiError::ItemNotFound)
    }

    pub async fn market_orders(
        &self,
        _region_id: i32,
        _type_id: i32,
        _order_type: EsiOrderType,
    ) -> Result<Vec<EsiMarketOrder>, EsiError> {
        Ok(Vec::new())
    }

    pub async fn market_history(
        &self,
        _region_id: i32,
        _type_id: i32,
    ) -> Result<Vec<EsiMarketHistoryDay>, EsiError> {
        Ok(Vec::new())
    }
}
