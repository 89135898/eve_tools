use crate::{
    attention_score, liquidity_score, net_profit, DataQuality, FeeProfile, OrderBookSummary,
    JITA_4_4_STATION_ID,
};
use rust_decimal::Decimal;

#[derive(Clone, Debug, PartialEq)]
pub struct PublicMarketOrder {
    pub type_id: i32,
    pub location_id: i64,
    pub is_buy_order: bool,
    pub price: Decimal,
    pub volume_remain: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicMarketHistoryDay {
    pub average: Decimal,
    pub date: String,
    pub volume: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PriceTrend {
    Up,
    Down,
    Stable,
}

impl PriceTrend {
    pub fn as_code(&self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Stable => "stable",
        }
    }
}

pub fn summarize_jita_market(
    type_id: i32,
    item_name: impl Into<String>,
    orders: &[PublicMarketOrder],
    history: &[PublicMarketHistoryDay],
    last_synced_at: impl Into<String>,
) -> OrderBookSummary {
    summarize_station_market(
        JITA_4_4_STATION_ID,
        type_id,
        item_name,
        orders,
        history,
        last_synced_at,
    )
}

pub fn summarize_station_market(
    station_id: i64,
    type_id: i32,
    item_name: impl Into<String>,
    orders: &[PublicMarketOrder],
    history: &[PublicMarketHistoryDay],
    last_synced_at: impl Into<String>,
) -> OrderBookSummary {
    let station_orders = orders
        .iter()
        .filter(|order| order.location_id == station_id && order.type_id == type_id);

    let best_bid = station_orders
        .clone()
        .filter(|order| order.is_buy_order)
        .map(|order| order.price)
        .max()
        .unwrap_or(Decimal::ZERO);

    let best_ask = station_orders
        .clone()
        .filter(|order| !order.is_buy_order)
        .map(|order| order.price)
        .min()
        .unwrap_or(Decimal::ZERO);

    let top_buy_depth = if best_bid > Decimal::ZERO {
        station_orders
            .clone()
            .filter(|order| order.is_buy_order && order.price == best_bid)
            .map(|order| order.volume_remain)
            .sum()
    } else {
        0
    };

    let top_sell_depth = if best_ask > Decimal::ZERO {
        station_orders
            .clone()
            .filter(|order| !order.is_buy_order && order.price == best_ask)
            .map(|order| order.volume_remain)
            .sum()
    } else {
        0
    };

    let daily_volume = history.last().map(|day| day.volume).unwrap_or(0);

    OrderBookSummary {
        type_id,
        item_name: item_name.into(),
        best_bid,
        best_ask,
        daily_volume,
        top_buy_depth,
        top_sell_depth,
        last_synced_at: last_synced_at.into(),
    }
}

pub fn classify_price_trend(history: &[PublicMarketHistoryDay]) -> PriceTrend {
    if history.len() < 2 {
        return PriceTrend::Stable;
    }

    let previous = history[history.len() - 2].average;
    let current = history[history.len() - 1].average;

    if previous <= Decimal::ZERO {
        return PriceTrend::Stable;
    }

    let one_percent = Decimal::new(1, 2);
    let change_ratio = (current - previous) / previous;

    if change_ratio > one_percent {
        PriceTrend::Up
    } else if change_ratio < -one_percent {
        PriceTrend::Down
    } else {
        PriceTrend::Stable
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CandidateAnalysis {
    pub type_id: i32,
    pub item_name: String,
    pub recommended_entry_price: Decimal,
    pub recommended_exit_price: Decimal,
    pub net_profit: Decimal,
    pub attention_score: u8,
    pub liquidity_score: u8,
    pub confidence_score: u8,
    pub reason_codes: Vec<String>,
}

pub fn build_selection_candidate(
    summary: &OrderBookSummary,
    fee: &FeeProfile,
) -> CandidateAnalysis {
    let one_cent = Decimal::new(1, 2);
    let recommended_entry_price = if summary.best_bid > Decimal::ZERO {
        summary.best_bid + one_cent
    } else {
        Decimal::ZERO
    };
    let recommended_exit_price = if summary.best_ask > one_cent {
        summary.best_ask - one_cent
    } else {
        Decimal::ZERO
    };

    let net_profit_value =
        if recommended_entry_price > Decimal::ZERO && recommended_exit_price > Decimal::ZERO {
            net_profit(recommended_entry_price, recommended_exit_price, fee)
        } else {
            Decimal::ZERO
        };

    let net_margin_pct = if recommended_entry_price > Decimal::ZERO {
        (net_profit_value / recommended_entry_price) * Decimal::from(100)
    } else {
        Decimal::ZERO
    };

    let top_depth = summary.top_buy_depth.min(summary.top_sell_depth);
    let liquidity_score_value = liquidity_score(summary.daily_volume, top_depth);
    let attention_score_value = attention_score(net_margin_pct, summary.daily_volume, top_depth);
    let reason_codes = candidate_reason_codes(
        summary,
        net_profit_value,
        summary.spread_percent(),
        top_depth,
    );
    let confidence_score_value = confidence_score(summary, liquidity_score_value, net_profit_value);

    CandidateAnalysis {
        type_id: summary.type_id,
        item_name: summary.item_name.clone(),
        recommended_entry_price,
        recommended_exit_price,
        net_profit: net_profit_value,
        attention_score: attention_score_value,
        liquidity_score: liquidity_score_value,
        confidence_score: confidence_score_value,
        reason_codes,
    }
}

fn confidence_score(
    summary: &OrderBookSummary,
    liquidity_score: u8,
    net_profit_value: Decimal,
) -> u8 {
    let quality_score = match summary.data_quality() {
        DataQuality::Fresh => 100u16,
        DataQuality::Sparse => 45u16,
        DataQuality::Missing => 0u16,
        DataQuality::Stale => 35u16,
    };
    let profit_score = if net_profit_value > Decimal::ZERO {
        100u16
    } else {
        20u16
    };

    ((quality_score * 50 + liquidity_score as u16 * 30 + profit_score * 20) / 100) as u8
}

fn candidate_reason_codes(
    summary: &OrderBookSummary,
    net_profit_value: Decimal,
    spread_pct: Decimal,
    top_depth: u64,
) -> Vec<String> {
    let mut reasons = Vec::new();
    match summary.data_quality() {
        DataQuality::Sparse => reasons.push("sparse_market_data".to_string()),
        DataQuality::Missing => reasons.push("missing_market_side".to_string()),
        DataQuality::Stale => reasons.push("stale_market_data".to_string()),
        DataQuality::Fresh => {}
    }

    if spread_pct >= Decimal::new(5, 0) && net_profit_value > Decimal::ZERO {
        reasons.push("healthy_spread".to_string());
    } else if spread_pct >= Decimal::new(2, 0) {
        reasons.push("acceptable_spread".to_string());
    }
    if summary.daily_volume >= 1_000_000 {
        reasons.push("high_daily_volume".to_string());
    } else if summary.daily_volume >= 1_000 {
        reasons.push("moderate_velocity".to_string());
    }
    if top_depth >= 100_000 {
        reasons.push("deep_top_book".to_string());
    }
    if net_profit_value <= Decimal::ZERO {
        reasons.push("negative_net_profit".to_string());
    }

    reasons
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FeeProfile, JITA_4_4_STATION_ID};
    use rust_decimal::Decimal;

    fn order(
        type_id: i32,
        is_buy_order: bool,
        price: Decimal,
        volume_remain: u64,
        location_id: i64,
    ) -> PublicMarketOrder {
        PublicMarketOrder {
            type_id,
            location_id,
            is_buy_order,
            price,
            volume_remain,
        }
    }

    #[test]
    fn summarizes_jita_top_of_book_and_ignores_other_locations() {
        let orders = vec![
            order(34, true, Decimal::new(501, 2), 500_000, JITA_4_4_STATION_ID),
            order(34, true, Decimal::new(501, 2), 125_000, JITA_4_4_STATION_ID),
            order(34, true, Decimal::new(502, 2), 999_000, 60008494),
            order(
                34,
                false,
                Decimal::new(549, 2),
                620_000,
                JITA_4_4_STATION_ID,
            ),
            order(34, false, Decimal::new(549, 2), 30_000, JITA_4_4_STATION_ID),
            order(34, false, Decimal::new(548, 2), 900_000, 60008494),
        ];
        let history = vec![
            PublicMarketHistoryDay {
                average: Decimal::new(510, 2),
                date: "2026-05-24".to_string(),
                volume: 1_000_000,
            },
            PublicMarketHistoryDay {
                average: Decimal::new(518, 2),
                date: "2026-05-25".to_string(),
                volume: 1_250_000,
            },
        ];

        let summary =
            summarize_jita_market(34, "Tritanium", &orders, &history, "2026-05-25T12:00:00Z");

        assert_eq!(summary.type_id, 34);
        assert_eq!(summary.item_name, "Tritanium");
        assert_eq!(summary.best_bid, Decimal::new(501, 2));
        assert_eq!(summary.best_ask, Decimal::new(549, 2));
        assert_eq!(summary.top_buy_depth, 625_000);
        assert_eq!(summary.top_sell_depth, 650_000);
        assert_eq!(summary.daily_volume, 1_250_000);
        assert_eq!(classify_price_trend(&history), PriceTrend::Up);
    }

    #[test]
    fn summarizes_configured_station_top_of_book() {
        let station_id = 60008494;
        let orders = vec![
            order(34, true, Decimal::new(501, 2), 500_000, JITA_4_4_STATION_ID),
            order(34, true, Decimal::new(640, 2), 80_000, station_id),
            order(34, true, Decimal::new(640, 2), 20_000, station_id),
            order(34, false, Decimal::new(705, 2), 70_000, station_id),
            order(34, false, Decimal::new(710, 2), 90_000, station_id),
            order(35, true, Decimal::new(700, 2), 999_000, station_id),
        ];

        let summary = summarize_station_market(
            station_id,
            34,
            "Tritanium",
            &orders,
            &[],
            "2026-05-25T12:00:00Z",
        );

        assert_eq!(summary.best_bid, Decimal::new(640, 2));
        assert_eq!(summary.best_ask, Decimal::new(705, 2));
        assert_eq!(summary.top_buy_depth, 100_000);
        assert_eq!(summary.top_sell_depth, 70_000);
    }

    #[test]
    fn marks_missing_when_jita_lacks_one_side() {
        let orders = vec![order(
            34,
            true,
            Decimal::new(501, 2),
            10,
            JITA_4_4_STATION_ID,
        )];
        let summary = summarize_jita_market(34, "Tritanium", &orders, &[], "2026-05-25T12:00:00Z");

        assert_eq!(summary.best_bid, Decimal::new(501, 2));
        assert_eq!(summary.best_ask, Decimal::ZERO);
        assert_eq!(summary.data_quality(), crate::DataQuality::Missing);
    }

    #[test]
    fn price_trend_uses_one_percent_threshold() {
        let stable = vec![
            PublicMarketHistoryDay {
                average: Decimal::new(10000, 2),
                date: "2026-05-24".to_string(),
                volume: 100,
            },
            PublicMarketHistoryDay {
                average: Decimal::new(10050, 2),
                date: "2026-05-25".to_string(),
                volume: 100,
            },
        ];
        let down = vec![
            PublicMarketHistoryDay {
                average: Decimal::new(10000, 2),
                date: "2026-05-24".to_string(),
                volume: 100,
            },
            PublicMarketHistoryDay {
                average: Decimal::new(9800, 2),
                date: "2026-05-25".to_string(),
                volume: 100,
            },
        ];

        assert_eq!(classify_price_trend(&stable), PriceTrend::Stable);
        assert_eq!(classify_price_trend(&down), PriceTrend::Down);
        assert_eq!(classify_price_trend(&[]), PriceTrend::Stable);
    }

    #[test]
    fn price_trend_treats_exactly_one_percent_as_stable() {
        let up_exactly = vec![
            PublicMarketHistoryDay {
                average: Decimal::new(10000, 2),
                date: "2026-05-24".to_string(),
                volume: 100,
            },
            PublicMarketHistoryDay {
                average: Decimal::new(10100, 2),
                date: "2026-05-25".to_string(),
                volume: 100,
            },
        ];
        let down_exactly = vec![
            PublicMarketHistoryDay {
                average: Decimal::new(10000, 2),
                date: "2026-05-24".to_string(),
                volume: 100,
            },
            PublicMarketHistoryDay {
                average: Decimal::new(9900, 2),
                date: "2026-05-25".to_string(),
                volume: 100,
            },
        ];

        assert_eq!(classify_price_trend(&up_exactly), PriceTrend::Stable);
        assert_eq!(classify_price_trend(&down_exactly), PriceTrend::Stable);
    }

    #[test]
    fn ignores_other_type_orders_even_at_same_station() {
        let orders = vec![
            order(34, true, Decimal::new(501, 2), 500_000, JITA_4_4_STATION_ID),
            order(
                34,
                false,
                Decimal::new(549, 2),
                620_000,
                JITA_4_4_STATION_ID,
            ),
            order(
                35,
                true,
                Decimal::new(999, 2),
                9_999_999,
                JITA_4_4_STATION_ID,
            ),
            order(
                35,
                false,
                Decimal::new(100, 2),
                8_888_888,
                JITA_4_4_STATION_ID,
            ),
        ];

        let summary = summarize_jita_market(34, "Tritanium", &orders, &[], "2026-05-25T12:00:00Z");

        assert_eq!(summary.best_bid, Decimal::new(501, 2));
        assert_eq!(summary.best_ask, Decimal::new(549, 2));
        assert_eq!(summary.top_buy_depth, 500_000);
        assert_eq!(summary.top_sell_depth, 620_000);
    }

    #[test]
    fn builds_selection_candidate_from_summary_and_fee_profile() {
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

        let candidate = build_selection_candidate(&summary, &FeeProfile::conservative_default());

        assert_eq!(candidate.type_id, 34);
        assert_eq!(candidate.item_name, "Tritanium");
        assert_eq!(candidate.recommended_entry_price, Decimal::new(502, 2));
        assert_eq!(candidate.recommended_exit_price, Decimal::new(548, 2));
        assert!(candidate.net_profit > Decimal::ZERO);
        assert_eq!(candidate.attention_score, 73);
        assert!(candidate
            .reason_codes
            .contains(&"healthy_spread".to_string()));
        assert!(candidate
            .reason_codes
            .contains(&"high_daily_volume".to_string()));
        assert!(candidate
            .reason_codes
            .contains(&"deep_top_book".to_string()));
    }

    #[test]
    fn candidate_reasons_explain_sparse_or_missing_data() {
        let summary = OrderBookSummary {
            type_id: 999,
            item_name: "Slow Item".to_string(),
            best_bid: Decimal::new(100, 2),
            best_ask: Decimal::new(101, 2),
            daily_volume: 3,
            top_buy_depth: 1,
            top_sell_depth: 1,
            last_synced_at: "2026-05-25T12:00:00Z".to_string(),
        };

        let candidate = build_selection_candidate(&summary, &FeeProfile::conservative_default());

        assert!(candidate
            .reason_codes
            .contains(&"sparse_market_data".to_string()));
        assert!(candidate.attention_score < 40);
    }

    #[test]
    fn attention_score_uses_fee_adjusted_net_margin() {
        let summary = OrderBookSummary {
            type_id: 1001,
            item_name: "Margin Item".to_string(),
            best_bid: Decimal::new(1000, 2),
            best_ask: Decimal::new(1200, 2),
            daily_volume: 10_000,
            top_buy_depth: 1_000,
            top_sell_depth: 1_000,
            last_synced_at: "2026-05-25T12:00:00Z".to_string(),
        };
        let zero_fee = FeeProfile {
            sales_tax_rate: Decimal::ZERO,
            broker_fee_rate: Decimal::ZERO,
            order_modification_fee: Decimal::ZERO,
        };
        let heavy_fee = FeeProfile {
            sales_tax_rate: Decimal::new(40, 2),
            broker_fee_rate: Decimal::new(40, 2),
            order_modification_fee: Decimal::new(50, 2),
        };

        let candidate_low_fee = build_selection_candidate(&summary, &zero_fee);
        let candidate_high_fee = build_selection_candidate(&summary, &heavy_fee);

        assert!(candidate_low_fee.attention_score > candidate_high_fee.attention_score);
    }

    #[test]
    fn confidence_score_follows_planned_formula() {
        let summary = OrderBookSummary {
            type_id: 1002,
            item_name: "Sparse Item".to_string(),
            best_bid: Decimal::new(1000, 2),
            best_ask: Decimal::new(1100, 2),
            daily_volume: 10,
            top_buy_depth: 10,
            top_sell_depth: 10,
            last_synced_at: "2026-05-25T12:00:00Z".to_string(),
        };
        let heavy_fee = FeeProfile {
            sales_tax_rate: Decimal::new(50, 2),
            broker_fee_rate: Decimal::new(50, 2),
            order_modification_fee: Decimal::new(100, 2),
        };

        let candidate = build_selection_candidate(&summary, &heavy_fee);

        // quality=100(fresh), liquidity=30, profit=20 => (100*50+30*30+20*20)/100 = 63
        assert_eq!(candidate.confidence_score, 63);
    }

    #[test]
    fn candidate_reasons_follow_planned_mapping() {
        let missing_summary = OrderBookSummary {
            type_id: 1003,
            item_name: "Missing Side".to_string(),
            best_bid: Decimal::new(1000, 2),
            best_ask: Decimal::ZERO,
            daily_volume: 5_000,
            top_buy_depth: 10_000,
            top_sell_depth: 0,
            last_synced_at: "2026-05-25T12:00:00Z".to_string(),
        };
        let moderate_summary = OrderBookSummary {
            type_id: 1004,
            item_name: "Moderate Velocity".to_string(),
            best_bid: Decimal::new(1000, 2),
            best_ask: Decimal::new(1030, 2),
            daily_volume: 20_000,
            top_buy_depth: 50_000,
            top_sell_depth: 60_000,
            last_synced_at: "2026-05-25T12:00:00Z".to_string(),
        };
        let acceptable_spread_summary = OrderBookSummary {
            type_id: 1005,
            item_name: "Acceptable Spread".to_string(),
            best_bid: Decimal::new(1000, 2),
            best_ask: Decimal::new(1025, 2),
            daily_volume: 2_000,
            top_buy_depth: 2_000,
            top_sell_depth: 2_000,
            last_synced_at: "2026-05-25T12:00:00Z".to_string(),
        };
        let heavy_fee = FeeProfile {
            sales_tax_rate: Decimal::new(40, 2),
            broker_fee_rate: Decimal::new(40, 2),
            order_modification_fee: Decimal::new(100, 2),
        };

        let missing =
            build_selection_candidate(&missing_summary, &FeeProfile::conservative_default());
        let moderate =
            build_selection_candidate(&moderate_summary, &FeeProfile::conservative_default());
        let acceptable = build_selection_candidate(&acceptable_spread_summary, &heavy_fee);

        assert!(missing
            .reason_codes
            .contains(&"missing_market_side".to_string()));
        assert!(!missing
            .reason_codes
            .contains(&"sparse_market_data".to_string()));

        assert!(moderate
            .reason_codes
            .contains(&"moderate_velocity".to_string()));

        assert!(acceptable
            .reason_codes
            .contains(&"acceptable_spread".to_string()));
        assert!(acceptable
            .reason_codes
            .contains(&"negative_net_profit".to_string()));
    }
}
