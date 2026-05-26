pub mod fixtures;
pub mod market;
pub mod market_analysis;
pub mod scoring;
pub mod views;

pub use market::{DataQuality, JITA_4_4_STATION_ID, OrderBookSummary, THE_FORGE_REGION_ID};
pub use market_analysis::{
    build_selection_candidate, classify_price_trend, summarize_jita_market, CandidateAnalysis,
    PriceTrend, PublicMarketHistoryDay, PublicMarketOrder,
};
pub use scoring::{attention_score, gross_spread, liquidity_score, net_profit, FeeProfile};
pub use views::{MarketLookupView, OrderMonitorView, SelectionCandidateView};
