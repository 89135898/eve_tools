use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketLookupView {
    pub type_id: i32,
    pub item_name: String,
    pub best_bid: String,
    pub best_ask: String,
    pub spread: String,
    pub spread_percent: String,
    pub daily_volume: u64,
    pub price_trend: String,
    pub top_buy_depth: u64,
    pub top_sell_depth: u64,
    pub last_synced_at: String,
    pub data_quality: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionCandidateView {
    pub type_id: i32,
    pub item_name: String,
    pub recommended_entry_price: String,
    pub recommended_exit_price: String,
    pub net_profit: String,
    pub attention_score: u8,
    pub liquidity_score: u8,
    pub confidence_score: u8,
    pub reason_codes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderMonitorView {
    pub order_id: String,
    pub type_id: i32,
    pub item_name: String,
    pub side: String,
    pub current_price: String,
    pub market_leader_price: String,
    pub recommended_price: String,
    pub recommended_action: String,
    pub urgency_score: u8,
    pub reason_codes: Vec<String>,
    pub stale_data_flag: bool,
}
