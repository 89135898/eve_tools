use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FeeProfile {
    pub sales_tax_rate: Decimal,
    pub broker_fee_rate: Decimal,
    pub order_modification_fee: Decimal,
}

impl FeeProfile {
    pub fn conservative_default() -> Self {
        Self {
            sales_tax_rate: Decimal::new(36, 3),
            broker_fee_rate: Decimal::new(30, 3),
            order_modification_fee: Decimal::ZERO,
        }
    }
}

pub fn gross_spread(best_bid: Decimal, best_ask: Decimal) -> Decimal {
    best_ask - best_bid
}

pub fn net_profit(best_bid: Decimal, best_ask: Decimal, fee: &FeeProfile) -> Decimal {
    let sale_after_tax = best_ask * (Decimal::ONE - fee.sales_tax_rate);
    let buy_with_broker = best_bid * (Decimal::ONE + fee.broker_fee_rate);
    sale_after_tax - buy_with_broker - fee.order_modification_fee
}

pub fn liquidity_score(daily_volume: u64, top_depth: u64) -> u8 {
    let volume_score = match daily_volume {
        0..=9 => 5,
        10..=99 => 25,
        100..=999 => 55,
        1_000..=9_999 => 80,
        _ => 100,
    };
    let depth_score = match top_depth {
        0..=4 => 10,
        5..=24 => 35,
        25..=99 => 60,
        100..=999 => 80,
        _ => 100,
    };
    ((volume_score + depth_score) / 2) as u8
}

pub fn attention_score(net_profit_margin_pct: Decimal, daily_volume: u64, top_depth: u64) -> u8 {
    let margin_score = if net_profit_margin_pct < Decimal::ZERO {
        0
    } else if net_profit_margin_pct < Decimal::new(2, 0) {
        25
    } else if net_profit_margin_pct < Decimal::new(5, 0) {
        55
    } else if net_profit_margin_pct < Decimal::new(12, 0) {
        80
    } else {
        100
    };
    let liquidity = liquidity_score(daily_volume, top_depth);
    ((margin_score as u16 * 60 + liquidity as u16 * 40) / 100) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn net_profit_subtracts_tax_broker_and_modification_fee() {
        let fee = FeeProfile {
            sales_tax_rate: Decimal::new(10, 2),
            broker_fee_rate: Decimal::new(5, 2),
            order_modification_fee: Decimal::new(25, 0),
        };

        let result = net_profit(Decimal::new(1000, 0), Decimal::new(1400, 0), &fee);

        assert_eq!(result, Decimal::new(185, 0));
    }

    #[test]
    fn liquidity_score_rejects_dead_items() {
        assert!(liquidity_score(1, 1) < 20);
        assert!(liquidity_score(2_500, 250) >= 80);
    }

    #[test]
    fn attention_score_balances_margin_and_liquidity() {
        let strong = attention_score(Decimal::new(8, 0), 2_500, 250);
        let weak = attention_score(Decimal::new(20, 0), 2, 1);

        assert!(strong > weak);
    }
}
