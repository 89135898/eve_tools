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
