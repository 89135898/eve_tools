use evetools_esi::{EsiMarketHistoryDay, EsiMarketOrder, EsiTypeInfo, UniverseIdsResponse};

#[test]
fn parses_market_orders_response() {
    let json = include_str!("fixtures/market_orders.json");
    let orders: Vec<EsiMarketOrder> = serde_json::from_str(json).unwrap();

    assert_eq!(orders.len(), 2);
    assert_eq!(orders[0].type_id, 34);
    assert!(orders[0].is_buy_order);
    assert_eq!(orders[0].location_id, 60003760);
    assert_eq!(orders[0].volume_remain, 500_000);
    assert_eq!(orders[1].price, 5.49);
}

#[test]
fn parses_market_history_response() {
    let json = include_str!("fixtures/market_history.json");
    let history: Vec<EsiMarketHistoryDay> = serde_json::from_str(json).unwrap();

    assert_eq!(history.len(), 2);
    assert_eq!(history[1].date, "2026-05-25");
    assert_eq!(history[1].volume, 1_250_000);
    assert_eq!(history[1].average, 5.18);
}

#[test]
fn parses_universe_ids_response() {
    let json = include_str!("fixtures/universe_ids.json");
    let response: UniverseIdsResponse = serde_json::from_str(json).unwrap();

    let entry = response.inventory_types.unwrap().remove(0);
    assert_eq!(entry.id, 34);
    assert_eq!(entry.name, "Tritanium");
}

#[test]
fn parses_type_info_response() {
    let json = include_str!("fixtures/type_info.json");
    let response: EsiTypeInfo = serde_json::from_str(json).unwrap();

    assert_eq!(response.type_id, 34);
    assert_eq!(response.name, "Tritanium");
    assert!(response.published);
    assert_eq!(response.market_group_id, Some(1857));
}
