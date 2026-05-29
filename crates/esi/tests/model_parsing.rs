use evetools_esi::{
    EsiCharacterOrder, EsiMarketHistoryDay, EsiMarketOrder, EsiTokenResponse, EsiTypeInfo,
    UniverseIdsResponse,
};

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

#[test]
fn parses_character_orders_response() {
    let json = include_str!("fixtures/character_orders.json");
    let orders: Vec<EsiCharacterOrder> = serde_json::from_str(json).unwrap();

    assert_eq!(orders.len(), 2);
    assert_eq!(orders[0].order_id, 8_000_000_001);
    assert_eq!(orders[0].region_id, 10000002);
    assert_eq!(orders[0].location_id, 60003760);
    assert_eq!(orders[0].type_id, 34);
    assert!(!orders[0].is_buy_order);
    assert_eq!(orders[0].escrow, Some(0.0));
    assert!(orders[1].is_buy_order);
    assert_eq!(orders[1].escrow, Some(120000.0));
}

#[test]
fn parses_token_response() {
    let json = r#"{
      "access_token": "access-token",
      "expires_in": 1199,
      "token_type": "Bearer",
      "refresh_token": "refresh-token"
    }"#;

    let response: EsiTokenResponse = serde_json::from_str(json).unwrap();

    assert_eq!(response.access_token, "access-token");
    assert_eq!(response.expires_in, 1199);
    assert_eq!(response.token_type, "Bearer");
    assert_eq!(response.refresh_token.as_deref(), Some("refresh-token"));
}
