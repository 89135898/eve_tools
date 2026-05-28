use crate::views::{MarketLookupView, OrderMonitorView, SelectionCandidateView};

pub fn fixture_market_lookup(query: &str) -> MarketLookupView {
    let normalized = if query.trim().is_empty() {
        "Tritanium"
    } else {
        query.trim()
    };

    MarketLookupView {
        type_id: 34,
        item_name: normalized.to_string(),
        best_bid: "5.00".to_string(),
        best_ask: "5.50".to_string(),
        spread: "0.50".to_string(),
        spread_percent: "10.00".to_string(),
        daily_volume: 1_250_000,
        price_trend: "stable".to_string(),
        top_buy_depth: 500_000,
        top_sell_depth: 620_000,
        last_synced_at: "2026-05-25T12:00:00Z".to_string(),
        data_quality: "fresh".to_string(),
    }
}

pub fn fixture_selection_candidates() -> Vec<SelectionCandidateView> {
    vec![
        SelectionCandidateView {
            hub_id: "jita".to_string(),
            hub_name: "Jita".to_string(),
            region_id: crate::THE_FORGE_REGION_ID,
            station_id: crate::JITA_4_4_STATION_ID,
            type_id: 34,
            item_name: "Tritanium".to_string(),
            recommended_entry_price: "5.01".to_string(),
            recommended_exit_price: "5.49".to_string(),
            net_profit: "0.23".to_string(),
            attention_score: 82,
            liquidity_score: 96,
            confidence_score: 88,
            reason_codes: vec![
                "healthy_spread".to_string(),
                "high_daily_volume".to_string(),
                "deep_top_book".to_string(),
            ],
            last_synced_at: "2026-05-25T12:00:00Z".to_string(),
        },
        SelectionCandidateView {
            hub_id: "jita".to_string(),
            hub_name: "Jita".to_string(),
            region_id: crate::THE_FORGE_REGION_ID,
            station_id: crate::JITA_4_4_STATION_ID,
            type_id: 35,
            item_name: "Pyerite".to_string(),
            recommended_entry_price: "11.20".to_string(),
            recommended_exit_price: "12.05".to_string(),
            net_profit: "0.34".to_string(),
            attention_score: 68,
            liquidity_score: 77,
            confidence_score: 71,
            reason_codes: vec![
                "acceptable_spread".to_string(),
                "moderate_velocity".to_string(),
            ],
            last_synced_at: "2026-05-25T12:00:00Z".to_string(),
        },
    ]
}

pub fn fixture_order_monitor() -> Vec<OrderMonitorView> {
    vec![
        OrderMonitorView {
            order_id: "9000000001".to_string(),
            type_id: 34,
            item_name: "Tritanium".to_string(),
            side: "sell".to_string(),
            current_price: "5.60".to_string(),
            market_leader_price: "5.50".to_string(),
            recommended_price: "5.49".to_string(),
            recommended_action: "lower".to_string(),
            urgency_score: 91,
            reason_codes: vec![
                "undercut_detected".to_string(),
                "high_velocity_item".to_string(),
            ],
            stale_data_flag: false,
        },
        OrderMonitorView {
            order_id: "9000000002".to_string(),
            type_id: 35,
            item_name: "Pyerite".to_string(),
            side: "buy".to_string(),
            current_price: "11.10".to_string(),
            market_leader_price: "11.20".to_string(),
            recommended_price: "11.21".to_string(),
            recommended_action: "raise".to_string(),
            urgency_score: 76,
            reason_codes: vec!["overbid_detected".to_string()],
            stale_data_flag: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_include_all_three_mvp_views() {
        assert_eq!(fixture_market_lookup("Tritanium").item_name, "Tritanium");
        assert_eq!(fixture_selection_candidates().len(), 2);
        assert_eq!(fixture_order_monitor().len(), 2);
    }
}
