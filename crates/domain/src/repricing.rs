use crate::OrderMonitorView;
use rust_decimal::Decimal;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharacterOrderForRepricing {
    pub order_id: i64,
    pub type_id: i32,
    pub item_name: String,
    pub is_buy_order: bool,
    pub price: Decimal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarketPriceReference {
    pub best_bid: Decimal,
    pub best_ask: Decimal,
    pub last_synced_at: String,
}

pub fn analyze_character_order_repricing(
    order: &CharacterOrderForRepricing,
    market: Option<MarketPriceReference>,
) -> OrderMonitorView {
    let side = if order.is_buy_order { "buy" } else { "sell" };
    let Some(market) = market else {
        return OrderMonitorView {
            order_id: order.order_id.to_string(),
            type_id: order.type_id,
            item_name: order.item_name.clone(),
            side: side.to_string(),
            current_price: format_isk(order.price),
            market_leader_price: "n/a".to_string(),
            recommended_price: format_isk(order.price),
            recommended_action: "hold".to_string(),
            urgency_score: 20,
            reason_codes: vec!["missing_market_data".to_string()],
            stale_data_flag: true,
        };
    };

    let (leader_price, recommended_price, recommended_action, reason_codes, urgency_score) =
        if order.is_buy_order {
            if order.price < market.best_bid {
                (
                    market.best_bid,
                    market.best_bid + price_step(),
                    "raise",
                    vec!["overbid_detected".to_string()],
                    80,
                )
            } else {
                (
                    market.best_bid,
                    order.price,
                    "hold",
                    vec!["already_best_price".to_string()],
                    10,
                )
            }
        } else if order.price > market.best_ask {
            (
                market.best_ask,
                market.best_ask - price_step(),
                "lower",
                vec!["undercut_detected".to_string()],
                80,
            )
        } else {
            (
                market.best_ask,
                order.price,
                "hold",
                vec!["already_best_price".to_string()],
                10,
            )
        };

    OrderMonitorView {
        order_id: order.order_id.to_string(),
        type_id: order.type_id,
        item_name: order.item_name.clone(),
        side: side.to_string(),
        current_price: format_isk(order.price),
        market_leader_price: format_isk(leader_price),
        recommended_price: format_isk(recommended_price),
        recommended_action: recommended_action.to_string(),
        urgency_score,
        reason_codes,
        stale_data_flag: false,
    }
}

fn format_isk(value: Decimal) -> String {
    value.round_dp(2).to_string()
}

fn price_step() -> Decimal {
    Decimal::new(1, 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    fn order(is_buy_order: bool, price: Decimal) -> CharacterOrderForRepricing {
        CharacterOrderForRepricing {
            order_id: 8_000_000_001,
            type_id: 34,
            item_name: "Tritanium".to_string(),
            is_buy_order,
            price,
        }
    }

    fn market() -> MarketPriceReference {
        MarketPriceReference {
            best_bid: Decimal::new(501, 2),
            best_ask: Decimal::new(549, 2),
            last_synced_at: "2026-05-29T10:10:00Z".to_string(),
        }
    }

    #[test]
    fn sell_order_above_best_ask_recommends_lower() {
        let view =
            analyze_character_order_repricing(&order(false, Decimal::new(560, 2)), Some(market()));

        assert_eq!(view.recommended_action, "lower");
        assert_eq!(view.market_leader_price, "5.49");
        assert_eq!(view.recommended_price, "5.48");
        assert_eq!(view.reason_codes, vec!["undercut_detected"]);
        assert_eq!(view.urgency_score, 80);
    }

    #[test]
    fn buy_order_below_best_bid_recommends_raise() {
        let view =
            analyze_character_order_repricing(&order(true, Decimal::new(495, 2)), Some(market()));

        assert_eq!(view.recommended_action, "raise");
        assert_eq!(view.market_leader_price, "5.01");
        assert_eq!(view.recommended_price, "5.02");
        assert_eq!(view.reason_codes, vec!["overbid_detected"]);
        assert_eq!(view.urgency_score, 80);
    }

    #[test]
    fn already_best_orders_recommend_hold() {
        let sell =
            analyze_character_order_repricing(&order(false, Decimal::new(549, 2)), Some(market()));
        let buy =
            analyze_character_order_repricing(&order(true, Decimal::new(501, 2)), Some(market()));

        assert_eq!(sell.recommended_action, "hold");
        assert_eq!(sell.recommended_price, "5.49");
        assert_eq!(sell.reason_codes, vec!["already_best_price"]);
        assert_eq!(buy.recommended_action, "hold");
        assert_eq!(buy.recommended_price, "5.01");
        assert_eq!(buy.reason_codes, vec!["already_best_price"]);
    }

    #[test]
    fn missing_public_market_data_recommends_hold() {
        let view = analyze_character_order_repricing(&order(false, Decimal::new(560, 2)), None);

        assert_eq!(view.recommended_action, "hold");
        assert_eq!(view.market_leader_price, "n/a");
        assert_eq!(view.recommended_price, "5.60");
        assert_eq!(view.reason_codes, vec!["missing_market_data"]);
        assert!(view.stale_data_flag);
    }
}
