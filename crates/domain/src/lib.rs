pub mod fixtures;
pub mod market;
pub mod market_analysis;
pub mod repricing;
pub mod scoring;
pub mod views;

pub use market::{DataQuality, OrderBookSummary, JITA_4_4_STATION_ID, THE_FORGE_REGION_ID};
pub use market_analysis::{
    build_selection_candidate, classify_price_trend, summarize_jita_market,
    summarize_station_market, CandidateAnalysis, PriceTrend, PublicMarketHistoryDay,
    PublicMarketOrder,
};
pub use repricing::{
    analyze_character_order_repricing, CharacterOrderForRepricing, MarketPriceReference,
};
pub use scoring::{attention_score, gross_spread, liquidity_score, net_profit, FeeProfile};
pub use views::{
    MarketLookupView, OrderMonitorView, SelectionCandidateHubView, SelectionCandidateView,
};
