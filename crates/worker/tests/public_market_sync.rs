use evetools_db::{connect_pool, migrate_catalog_schema, MarketRepository};
use evetools_esi::EsiClient;
use evetools_test_support::{guarded_database_url_from_env, reset_evetools_catalog_schema};
use evetools_worker::{
    default_trade_hubs, run_public_market_region_sync, sync_public_market_region_orders,
    PublicMarketSyncCliConfig,
};
use httpmock::prelude::*;

static POSTGRES_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn syncs_region_orders_for_configured_hub_stations() {
    let _guard = POSTGRES_TEST_LOCK.lock().await;
    let url = match guarded_database_url_from_env() {
        Ok(Some(url)) => url,
        Ok(None) => {
            eprintln!("skipping Postgres test: EVETOOLS_TEST_DATABASE_URL is not set");
            return;
        }
        Err(error) => panic!("{error}"),
    };
    let server = MockServer::start();
    let orders = server.mock(|when, then| {
        when.method(GET)
            .path("/latest/markets/10000002/orders/")
            .query_param("datasource", "tranquility")
            .query_param("order_type", "all")
            .query_param("page", "1");
        then.status(200)
            .header("content-type", "application/json")
            .header("X-Pages", "1")
            .body(include_str!("../../esi/tests/fixtures/market_orders.json"));
    });

    let pool = connect_pool(&url).await.unwrap();
    reset_evetools_catalog_schema(&pool).await.unwrap();
    migrate_catalog_schema(&pool).await.unwrap();
    let repository = MarketRepository::new(pool);
    let client = EsiClient::new(server.base_url());

    sync_public_market_region_orders(&repository, &client, 10000002, &default_trade_hubs())
        .await
        .unwrap();

    orders.assert();
    let stored_orders = repository
        .latest_station_orders(10000002, 60003760, 10)
        .await
        .unwrap();
    assert_eq!(stored_orders.len(), 2);
    assert!(stored_orders
        .iter()
        .all(|order| order.station_id == 60003760));
}

#[tokio::test]
async fn cli_runner_syncs_region_orders_with_configured_esi_base_url() {
    let _guard = POSTGRES_TEST_LOCK.lock().await;
    let url = match guarded_database_url_from_env() {
        Ok(Some(url)) => url,
        Ok(None) => {
            eprintln!("skipping Postgres test: EVETOOLS_TEST_DATABASE_URL is not set");
            return;
        }
        Err(error) => panic!("{error}"),
    };
    let server = MockServer::start();
    let orders = server.mock(|when, then| {
        when.method(GET)
            .path("/latest/markets/10000002/orders/")
            .query_param("datasource", "tranquility")
            .query_param("order_type", "all")
            .query_param("page", "1");
        then.status(200)
            .header("content-type", "application/json")
            .header("X-Pages", "1")
            .body(include_str!("../../esi/tests/fixtures/market_orders.json"));
    });

    let pool = connect_pool(&url).await.unwrap();
    reset_evetools_catalog_schema(&pool).await.unwrap();
    let summary = run_public_market_region_sync(PublicMarketSyncCliConfig {
        database_url: url,
        esi_base_url: Some(server.base_url()),
        region_id: 10000002,
    })
    .await
    .unwrap();

    orders.assert();
    assert_eq!(summary.region_id, 10000002);
    assert!(summary.sync_run_id > 0);
}
