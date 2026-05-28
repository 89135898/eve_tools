use crate::{DataQuality, OrderBookSummary, PriceTrend};
use rust_decimal::Decimal;
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
    pub hub_id: String,
    pub hub_name: String,
    pub region_id: i32,
    pub station_id: i64,
    pub type_id: i32,
    pub item_name: String,
    pub recommended_entry_price: String,
    pub recommended_exit_price: String,
    pub net_profit: String,
    pub attention_score: u8,
    pub liquidity_score: u8,
    pub confidence_score: u8,
    pub reason_codes: Vec<String>,
    pub last_synced_at: String,
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

fn format_isk(value: Decimal) -> String {
    value.round_dp(2).to_string()
}

fn data_quality_code(value: DataQuality) -> &'static str {
    match value {
        DataQuality::Fresh => "fresh",
        DataQuality::Stale => "stale",
        DataQuality::Sparse => "sparse",
        DataQuality::Missing => "missing",
    }
}

impl MarketLookupView {
    pub fn from_summary(summary: OrderBookSummary, trend: PriceTrend) -> Self {
        Self {
            type_id: summary.type_id,
            item_name: summary.item_name.clone(),
            best_bid: format_isk(summary.best_bid),
            best_ask: format_isk(summary.best_ask),
            spread: format_isk(summary.spread()),
            spread_percent: format_isk(summary.spread_percent()),
            daily_volume: summary.daily_volume,
            price_trend: trend.as_code().to_string(),
            top_buy_depth: summary.top_buy_depth,
            top_sell_depth: summary.top_sell_depth,
            last_synced_at: summary.last_synced_at.clone(),
            data_quality: data_quality_code(summary.data_quality()).to_string(),
        }
    }
}

impl SelectionCandidateView {
    pub fn from_analysis(analysis: crate::CandidateAnalysis) -> Self {
        Self::from_analysis_for_hub(
            analysis,
            SelectionCandidateHubView {
                hub_id: "jita".to_string(),
                hub_name: "Jita".to_string(),
                region_id: crate::THE_FORGE_REGION_ID,
                station_id: crate::JITA_4_4_STATION_ID,
                last_synced_at: String::new(),
            },
        )
    }

    pub fn from_analysis_for_hub(
        analysis: crate::CandidateAnalysis,
        hub: SelectionCandidateHubView,
    ) -> Self {
        Self {
            hub_id: hub.hub_id,
            hub_name: hub.hub_name,
            region_id: hub.region_id,
            station_id: hub.station_id,
            type_id: analysis.type_id,
            item_name: analysis.item_name,
            recommended_entry_price: format_isk(analysis.recommended_entry_price),
            recommended_exit_price: format_isk(analysis.recommended_exit_price),
            net_profit: format_isk(analysis.net_profit),
            attention_score: analysis.attention_score,
            liquidity_score: analysis.liquidity_score,
            confidence_score: analysis.confidence_score,
            reason_codes: analysis.reason_codes,
            last_synced_at: hub.last_synced_at,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionCandidateHubView {
    pub hub_id: String,
    pub hub_name: String,
    pub region_id: i32,
    pub station_id: i64,
    pub last_synced_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OrderBookSummary, PriceTrend};
    use rust_decimal::Decimal;

    #[test]
    fn market_lookup_view_formats_summary_values() {
        let summary = OrderBookSummary {
            type_id: 34,
            item_name: "Tritanium".to_string(),
            best_bid: Decimal::new(501, 2),
            best_ask: Decimal::new(549, 2),
            daily_volume: 1_250_000,
            top_buy_depth: 625_000,
            top_sell_depth: 650_000,
            last_synced_at: "2026-05-25T12:00:00Z".to_string(),
        };

        let view = MarketLookupView::from_summary(summary, PriceTrend::Up);

        assert_eq!(view.best_bid, "5.01");
        assert_eq!(view.best_ask, "5.49");
        assert_eq!(view.spread, "0.48");
        assert_eq!(view.spread_percent, "9.58");
        assert_eq!(view.price_trend, "up");
        assert_eq!(view.data_quality, "fresh");
    }

    #[test]
    fn selection_candidate_view_formats_analysis_values() {
        let analysis = crate::CandidateAnalysis {
            type_id: 34,
            item_name: "Tritanium".to_string(),
            recommended_entry_price: Decimal::new(502, 2),
            recommended_exit_price: Decimal::new(548, 2),
            net_profit: Decimal::new(20, 2),
            attention_score: 82,
            liquidity_score: 96,
            confidence_score: 88,
            reason_codes: vec!["healthy_spread".to_string()],
        };

        let view = SelectionCandidateView::from_analysis(analysis);

        assert_eq!(view.hub_id, "jita");
        assert_eq!(view.hub_name, "Jita");
        assert_eq!(view.region_id, crate::THE_FORGE_REGION_ID);
        assert_eq!(view.station_id, crate::JITA_4_4_STATION_ID);
        assert_eq!(view.recommended_entry_price, "5.02");
        assert_eq!(view.recommended_exit_price, "5.48");
        assert_eq!(view.net_profit, "0.20");
        assert_eq!(view.reason_codes, vec!["healthy_spread"]);
    }
}
