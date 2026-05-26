use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

pub const THE_FORGE_REGION_ID: i32 = 10000002;
pub const JITA_4_4_STATION_ID: i64 = 60003760;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataQuality {
    Fresh,
    Stale,
    Sparse,
    Missing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OrderBookSummary {
    pub type_id: i32,
    pub item_name: String,
    pub best_bid: Decimal,
    pub best_ask: Decimal,
    pub daily_volume: u64,
    pub top_buy_depth: u64,
    pub top_sell_depth: u64,
    pub last_synced_at: String,
}

impl OrderBookSummary {
    pub fn spread(&self) -> Decimal {
        self.best_ask - self.best_bid
    }

    pub fn spread_percent(&self) -> Decimal {
        if self.best_bid <= Decimal::ZERO {
            return Decimal::ZERO;
        }
        ((self.best_ask - self.best_bid) / self.best_bid) * Decimal::from(100)
    }

    pub fn data_quality(&self) -> DataQuality {
        if self.best_bid <= Decimal::ZERO || self.best_ask <= Decimal::ZERO {
            return DataQuality::Missing;
        }
        if self.daily_volume < 10 {
            return DataQuality::Sparse;
        }
        DataQuality::Fresh
    }

    pub fn rounded_spread_percent(&self) -> f64 {
        self.spread_percent().round_dp(2).to_f64().unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spread_percent_uses_best_bid_as_base() {
        let summary = OrderBookSummary {
            type_id: 34,
            item_name: "Tritanium".to_string(),
            best_bid: Decimal::new(500, 2),
            best_ask: Decimal::new(550, 2),
            daily_volume: 1_000_000,
            top_buy_depth: 50_000,
            top_sell_depth: 60_000,
            last_synced_at: "2026-05-25T12:00:00Z".to_string(),
        };

        assert_eq!(summary.spread(), Decimal::new(50, 2));
        assert_eq!(summary.rounded_spread_percent(), 10.0);
    }

    #[test]
    fn sparse_data_quality_requires_volume() {
        let summary = OrderBookSummary {
            type_id: 35,
            item_name: "Pyerite".to_string(),
            best_bid: Decimal::new(1000, 2),
            best_ask: Decimal::new(1300, 2),
            daily_volume: 3,
            top_buy_depth: 1,
            top_sell_depth: 1,
            last_synced_at: "2026-05-25T12:00:00Z".to_string(),
        };

        assert_eq!(summary.data_quality(), DataQuality::Sparse);
    }
}
