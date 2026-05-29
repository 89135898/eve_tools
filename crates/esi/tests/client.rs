use evetools_esi::{EsiClient, EsiError, EsiOrderType};
use httpmock::prelude::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

#[tokio::test]
async fn resolves_inventory_type_by_name() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/latest/universe/ids/")
            .query_param("datasource", "tranquility")
            .json_body(vec!["Tritanium"]);
        then.status(200)
            .header("content-type", "application/json")
            .body(include_str!("fixtures/universe_ids.json"));
    });

    let client = EsiClient::new(server.base_url());
    let resolved = client.resolve_inventory_type("Tritanium").await.unwrap();

    mock.assert();
    assert_eq!(resolved.type_id, 34);
    assert_eq!(resolved.name, "Tritanium");
}

#[tokio::test]
async fn resolves_inventory_type_by_numeric_id() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/latest/universe/types/34/")
            .query_param("datasource", "tranquility");
        then.status(200)
            .header("content-type", "application/json")
            .body(include_str!("fixtures/type_info.json"));
    });

    let client = EsiClient::new(server.base_url());
    let resolved = client.resolve_inventory_type("34").await.unwrap();

    mock.assert();
    assert_eq!(resolved.type_id, 34);
    assert_eq!(resolved.name, "Tritanium");
}

#[tokio::test]
async fn fetches_all_market_order_pages() {
    let server = MockServer::start();
    let page_one = server.mock(|when, then| {
        when.method(GET)
            .path("/latest/markets/10000002/orders/")
            .query_param("datasource", "tranquility")
            .query_param("order_type", "all")
            .query_param("type_id", "34")
            .query_param("page", "1");
        then.status(200)
            .header("content-type", "application/json")
            .header("X-Pages", "2")
            .body(include_str!("fixtures/market_orders.json"));
    });
    let page_two = server.mock(|when, then| {
        when.method(GET)
            .path("/latest/markets/10000002/orders/")
            .query_param("datasource", "tranquility")
            .query_param("order_type", "all")
            .query_param("type_id", "34")
            .query_param("page", "2");
        then.status(200)
            .header("content-type", "application/json")
            .header("X-Pages", "2")
            .body("[]");
    });

    let client = EsiClient::new(server.base_url());
    let orders = client
        .market_orders(10000002, 34, EsiOrderType::All)
        .await
        .unwrap();

    page_one.assert();
    page_two.assert();
    assert_eq!(orders.len(), 2);
    assert_eq!(orders[0].order_id, 7000000001);
}

#[tokio::test]
async fn fetches_region_market_order_pages_without_type_filter() {
    let server = MockServer::start();
    let page_one = server.mock(|when, then| {
        when.method(GET)
            .path("/latest/markets/10000002/orders/")
            .query_param("datasource", "tranquility")
            .query_param("order_type", "all")
            .query_param("page", "1");
        then.status(200)
            .header("content-type", "application/json")
            .header("X-Pages", "2")
            .body(include_str!("fixtures/market_orders.json"));
    });
    let page_two = server.mock(|when, then| {
        when.method(GET)
            .path("/latest/markets/10000002/orders/")
            .query_param("datasource", "tranquility")
            .query_param("order_type", "all")
            .query_param("page", "2");
        then.status(200)
            .header("content-type", "application/json")
            .header("X-Pages", "2")
            .body("[]");
    });

    let client = EsiClient::new(server.base_url());
    let orders = client
        .region_market_orders(10000002, EsiOrderType::All)
        .await
        .unwrap();

    page_one.assert();
    page_two.assert();
    assert_eq!(orders.len(), 2);
    assert_eq!(orders[0].type_id, 34);
}

#[tokio::test]
async fn fetches_market_history() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/latest/markets/10000002/history/")
            .query_param("datasource", "tranquility")
            .query_param("type_id", "34");
        then.status(200)
            .header("content-type", "application/json")
            .body(include_str!("fixtures/market_history.json"));
    });

    let client = EsiClient::new(server.base_url());
    let history = client.market_history(10000002, 34).await.unwrap();

    mock.assert();
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].volume, 1_250_000);
}

#[tokio::test]
async fn maps_not_found_status_to_item_not_found() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/latest/universe/types/999999/")
            .query_param("datasource", "tranquility");
        then.status(404)
            .header("content-type", "application/json")
            .body("{\"error\":\"not found\"}");
    });

    let client = EsiClient::new(server.base_url());
    let error = client.resolve_inventory_type("999999").await.unwrap_err();

    assert!(matches!(error, EsiError::ItemNotFound));
}

#[tokio::test]
async fn request_timeout_maps_to_http_error() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = [0; 1024];
        let _ = stream.read(&mut buffer);
        thread::sleep(Duration::from_millis(200));
        let _ = stream.write_all(
            b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 2\r\n\r\n{}",
        );
    });

    let client =
        EsiClient::with_request_timeout(format!("http://{address}"), Duration::from_millis(25));
    let error = client
        .resolve_inventory_type("Tritanium")
        .await
        .unwrap_err();

    assert!(matches!(error, EsiError::Http(_)));
    server.join().unwrap();
}

#[tokio::test]
async fn exchanges_authorization_code_with_pkce() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/v2/oauth/token")
            .header("content-type", "application/x-www-form-urlencoded")
            .body_includes("grant_type=authorization_code")
            .body_includes("client_id=client-123")
            .body_includes("code=auth-code")
            .body_includes("redirect_uri=http%3A%2F%2F127.0.0.1%3A17813%2Fcallback")
            .body_includes("code_verifier=verifier-123");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{"access_token":"access-token","expires_in":1199,"token_type":"Bearer","refresh_token":"refresh-token"}"#,
            );
    });

    let client = EsiClient::new("https://esi.evetech.net");
    let response = client
        .exchange_authorization_code(
            &server.base_url(),
            "client-123",
            "auth-code",
            "http://127.0.0.1:17813/callback",
            "verifier-123",
        )
        .await
        .unwrap();

    mock.assert();
    assert_eq!(response.access_token, "access-token");
    assert_eq!(response.refresh_token.as_deref(), Some("refresh-token"));
}

#[tokio::test]
async fn refreshes_access_token() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/v2/oauth/token")
            .body_includes("grant_type=refresh_token")
            .body_includes("client_id=client-123")
            .body_includes("refresh_token=refresh-token");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{"access_token":"new-access-token","expires_in":1199,"token_type":"Bearer","refresh_token":"new-refresh-token"}"#,
            );
    });

    let client = EsiClient::new("https://esi.evetech.net");
    let response = client
        .refresh_access_token(&server.base_url(), "client-123", "refresh-token")
        .await
        .unwrap();

    mock.assert();
    assert_eq!(response.access_token, "new-access-token");
    assert_eq!(response.refresh_token.as_deref(), Some("new-refresh-token"));
}

#[tokio::test]
async fn character_orders_use_bearer_token() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/latest/characters/90000001/orders/")
            .query_param("datasource", "tranquility")
            .header("authorization", "Bearer access-token");
        then.status(200)
            .header("content-type", "application/json")
            .body(include_str!("fixtures/character_orders.json"));
    });

    let client = EsiClient::new(server.base_url());
    let orders = client
        .character_orders(90_000_001, "access-token")
        .await
        .unwrap();

    mock.assert();
    assert_eq!(orders.len(), 2);
    assert_eq!(orders[0].order_id, 8_000_000_001);
}
