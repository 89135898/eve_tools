use evetools_db::{connect_pool, migrate_catalog_schema, MarketRepository};
use evetools_esi::EsiClient;
use evetools_test_support::{guarded_database_url_from_env, reset_evetools_catalog_schema};
use evetools_worker::{
    default_trade_hubs, default_trade_hubs_as_db_records, run_public_market_region_sync,
    sync_public_market_region_orders, PublicMarketSyncCliConfig,
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

    let summary = sync_public_market_region_orders(
        &repository,
        &client,
        10000002,
        &default_trade_hubs(),
        "test-worker",
        1200,
        None,
    )
    .await
    .unwrap();

    orders.assert();
    assert_eq!(summary.region_id, 10000002);
    assert_eq!(summary.status, "success");
    assert_eq!(summary.order_count, 2);
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
        region_id: Some(10000002),
        all_default_regions: false,
        started_by: "test-worker".to_string(),
        lease_ttl_seconds: 1200,
        max_age_seconds: None,
        json: false,
    })
    .await
    .unwrap();

    orders.assert();
    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].region_id, 10000002);
    assert_eq!(summary[0].status, "success");
    assert!(summary[0].sync_run_id.unwrap() > 0);
}

#[tokio::test]
async fn cli_runner_reports_lease_conflict_as_already_running_summary() {
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

    let pool = connect_pool(&url).await.unwrap();
    reset_evetools_catalog_schema(&pool).await.unwrap();
    migrate_catalog_schema(&pool).await.unwrap();
    let repository = MarketRepository::new(pool);
    let active = repository
        .try_start_sync_run(
            10000002,
            "public-esi",
            "test-worker",
            "test-lease",
            chrono::Duration::seconds(1200),
        )
        .await
        .unwrap();

    let summary = run_public_market_region_sync(PublicMarketSyncCliConfig {
        database_url: url,
        esi_base_url: Some(server.base_url()),
        region_id: Some(10000002),
        all_default_regions: false,
        started_by: "test-worker".to_string(),
        lease_ttl_seconds: 1200,
        max_age_seconds: None,
        json: false,
    })
    .await
    .unwrap();

    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].region_id, 10000002);
    assert_eq!(summary[0].status, "already-running");
    assert_eq!(summary[0].sync_run_id, active.sync_run_id);
}

#[tokio::test]
async fn cli_runner_skips_recent_region_when_max_age_allows_cached_success() {
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

    let pool = connect_pool(&url).await.unwrap();
    reset_evetools_catalog_schema(&pool).await.unwrap();
    migrate_catalog_schema(&pool).await.unwrap();
    let repository = MarketRepository::new(pool);
    repository
        .upsert_trade_hubs(&default_trade_hubs_as_db_records())
        .await
        .unwrap();
    let lease = repository
        .try_start_sync_run(
            10000002,
            "public-esi",
            "test-worker",
            "test-lease",
            chrono::Duration::seconds(1200),
        )
        .await
        .unwrap();
    let sync_run_id = lease.sync_run_id.unwrap();
    repository.mark_sync_run_running(sync_run_id).await.unwrap();
    repository
        .complete_sync_run(sync_run_id, 0, 5)
        .await
        .unwrap();

    let summary = run_public_market_region_sync(PublicMarketSyncCliConfig {
        database_url: url,
        esi_base_url: Some(server.base_url()),
        region_id: Some(10000002),
        all_default_regions: false,
        started_by: "test-worker".to_string(),
        lease_ttl_seconds: 1200,
        max_age_seconds: Some(3600),
        json: false,
    })
    .await
    .unwrap();

    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].region_id, 10000002);
    assert_eq!(summary[0].status, "skipped");
    assert_eq!(summary[0].sync_run_id, Some(sync_run_id));
    assert_eq!(summary[0].order_count, 5);
}
