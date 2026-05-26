use crate::{JITA_4_4_STATION_ID, OrderBookSummary};
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
    let jita_orders = orders
        .iter()
        .filter(|order| order.location_id == JITA_4_4_STATION_ID && order.type_id == type_id);

    let best_bid = jita_orders
        .clone()
        .filter(|order| order.is_buy_order)
        .map(|order| order.price)
        .max()
        .unwrap_or(Decimal::ZERO);

    let best_ask = jita_orders
        .clone()
        .filter(|order| !order.is_buy_order)
        .map(|order| order.price)
        .min()
        .unwrap_or(Decimal::ZERO);

    let top_buy_depth = if best_bid > Decimal::ZERO {
        jita_orders
            .clone()
            .filter(|order| order.is_buy_order && order.price == best_bid)
            .map(|order| order.volume_remain)
            .sum()
    } else {
        0
    };

    let top_sell_depth = if best_ask > Decimal::ZERO {
        jita_orders
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::JITA_4_4_STATION_ID;
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
            order(34, false, Decimal::new(549, 2), 620_000, JITA_4_4_STATION_ID),
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

        let summary = summarize_jita_market(
            34,
            "Tritanium",
            &orders,
            &history,
            "2026-05-25T12:00:00Z",
        );

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
    fn marks_missing_when_jita_lacks_one_side() {
        let orders = vec![order(
            34,
            true,
            Decimal::new(501, 2),
            10,
            JITA_4_4_STATION_ID,
        )];
        let summary = summarize_jita_market(
            34,
            "Tritanium",
            &orders,
            &[],
            "2026-05-25T12:00:00Z",
        );

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
            order(34, false, Decimal::new(549, 2), 620_000, JITA_4_4_STATION_ID),
            order(35, true, Decimal::new(999, 2), 9_999_999, JITA_4_4_STATION_ID),
            order(35, false, Decimal::new(100, 2), 8_888_888, JITA_4_4_STATION_ID),
        ];

        let summary = summarize_jita_market(
            34,
            "Tritanium",
            &orders,
            &[],
            "2026-05-25T12:00:00Z",
        );

        assert_eq!(summary.best_bid, Decimal::new(501, 2));
        assert_eq!(summary.best_ask, Decimal::new(549, 2));
        assert_eq!(summary.top_buy_depth, 500_000);
        assert_eq!(summary.top_sell_depth, 620_000);
    }
}
