use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EsiOrderType {
    All,
    Buy,
    Sell,
}

impl EsiOrderType {
    pub fn as_query_value(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Buy => "buy",
            Self::Sell => "sell",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EsiMarketOrder {
    pub duration: i32,
    pub is_buy_order: bool,
    pub issued: String,
    pub location_id: i64,
    pub min_volume: i32,
    pub order_id: i64,
    pub price: f64,
    pub range: String,
    pub system_id: i32,
    pub type_id: i32,
    pub volume_remain: i32,
    pub volume_total: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EsiMarketHistoryDay {
    pub average: f64,
    pub date: String,
    pub highest: f64,
    pub lowest: f64,
    pub order_count: i64,
    pub volume: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EsiTypeInfo {
    pub group_id: i32,
    pub market_group_id: Option<i32>,
    pub name: String,
    pub published: bool,
    pub type_id: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UniverseIdsResponse {
    pub inventory_types: Option<Vec<UniverseIdEntry>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UniverseIdEntry {
    pub id: i32,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedInventoryType {
    pub type_id: i32,
    pub name: String,
}
