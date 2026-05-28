use evetools_db::{
    connect_pool, migrate_catalog_schema, MarketOrderSnapshotInput, MarketRepository, TradeHub,
};
use evetools_test_support::{guarded_database_url_from_env, reset_evetools_catalog_schema};

static POSTGRES_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn jita_hub() -> TradeHub {
    TradeHub {
        hub_id: "jita".to_string(),
        display_name: "Jita".to_string(),
        region_id: 10000002,
        system_id: 30000142,
        station_id: 60003760,
        enabled: true,
        sort_order: 10,
    }
}

async fn prepare_market_repository() -> Option<MarketRepository> {
    let url = match guarded_database_url_from_env() {
        Ok(Some(url)) => url,
        Ok(None) => {
            eprintln!("skipping Postgres test: EVETOOLS_TEST_DATABASE_URL is not set");
            return None;
        }
        Err(error) => panic!("{error}"),
    };
    let pool = connect_pool(&url).await.unwrap();
    reset_evetools_catalog_schema(&pool).await.unwrap();
    migrate_catalog_schema(&pool).await.unwrap();
    Some(MarketRepository::new(pool))
}

fn tritanium_order(sync_run_id: i64) -> MarketOrderSnapshotInput {
    MarketOrderSnapshotInput {
        sync_run_id,
        region_id: 10000002,
        station_id: 60003760,
        type_id: 34,
        order_id: 7_000_000_001,
        is_buy_order: true,
        price: 5.01,
        volume_remain: 500_000,
        volume_total: 1_000_000,
        issued: "2026-05-25T11:45:00Z".to_string(),
        duration: 90,
        min_volume: 1,
        order_range: "station".to_string(),
        system_id: 30000142,
    }
}

#[tokio::test]
async fn persists_trade_hubs_sync_runs_and_latest_station_orders() {
    let _guard = POSTGRES_TEST_LOCK.lock().await;
    let Some(repository) = prepare_market_repository().await else {
        return;
    };

    repository.upsert_trade_hubs(&[jita_hub()]).await.unwrap();
    let hubs = repository.list_enabled_trade_hubs().await.unwrap();
    let jita = hubs.iter().find(|hub| hub.hub_id == "jita").unwrap();
    assert_eq!(jita.station_id, 60003760);

    let sync_run_id = repository
        .start_sync_run(10000002, "public-esi")
        .await
        .unwrap();
    repository
        .replace_order_snapshots(sync_run_id, &[tritanium_order(sync_run_id)])
        .await
        .unwrap();
    repository
        .complete_sync_run(sync_run_id, 2, 1)
        .await
        .unwrap();

    let orders = repository
        .latest_station_orders(10000002, 60003760, 10)
        .await
        .unwrap();
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0].type_id, 34);
    assert_eq!(orders[0].price, 5.01);
}

#[tokio::test]
async fn aggregates_latest_station_order_books_from_successful_sync() {
    let _guard = POSTGRES_TEST_LOCK.lock().await;
    let Some(repository) = prepare_market_repository().await else {
        return;
    };

    repository.upsert_trade_hubs(&[jita_hub()]).await.unwrap();

    let failed_run_id = repository
        .start_sync_run(10000002, "public-esi")
        .await
        .unwrap();
    repository
        .replace_order_snapshots(
            failed_run_id,
            &[MarketOrderSnapshotInput {
                sync_run_id: failed_run_id,
                region_id: 10000002,
                station_id: 60003760,
                type_id: 34,
                order_id: 7_000_000_000,
                is_buy_order: true,
                price: 999.0,
                volume_remain: 1,
                volume_total: 1,
                issued: "2026-05-25T11:00:00Z".to_string(),
                duration: 90,
                min_volume: 1,
                order_range: "station".to_string(),
                system_id: 30000142,
            }],
        )
        .await
        .unwrap();
    repository
        .fail_sync_run(failed_run_id, "fixture failure")
        .await
        .unwrap();

    let sync_run_id = repository
        .start_sync_run(10000002, "public-esi")
        .await
        .unwrap();
    repository
        .replace_order_snapshots(
            sync_run_id,
            &[
                MarketOrderSnapshotInput {
                    sync_run_id,
                    region_id: 10000002,
                    station_id: 60003760,
                    type_id: 34,
                    order_id: 7_000_000_001,
                    is_buy_order: true,
                    price: 5.01,
                    volume_remain: 500_000,
                    volume_total: 1_000_000,
                    issued: "2026-05-25T11:45:00Z".to_string(),
                    duration: 90,
                    min_volume: 1,
                    order_range: "station".to_string(),
                    system_id: 30000142,
                },
                MarketOrderSnapshotInput {
                    sync_run_id,
                    region_id: 10000002,
                    station_id: 60003760,
                    type_id: 34,
                    order_id: 7_000_000_002,
                    is_buy_order: true,
                    price: 5.01,
                    volume_remain: 125_000,
                    volume_total: 125_000,
                    issued: "2026-05-25T11:46:00Z".to_string(),
                    duration: 90,
                    min_volume: 1,
                    order_range: "station".to_string(),
                    system_id: 30000142,
                },
                MarketOrderSnapshotInput {
                    sync_run_id,
                    region_id: 10000002,
                    station_id: 60003760,
                    type_id: 34,
                    order_id: 7_000_000_003,
                    is_buy_order: false,
                    price: 5.49,
                    volume_remain: 650_000,
                    volume_total: 650_000,
                    issued: "2026-05-25T11:47:00Z".to_string(),
                    duration: 90,
                    min_volume: 1,
                    order_range: "station".to_string(),
                    system_id: 30000142,
                },
                MarketOrderSnapshotInput {
                    sync_run_id,
                    region_id: 10000002,
                    station_id: 60003760,
                    type_id: 35,
                    order_id: 7_000_000_004,
                    is_buy_order: true,
                    price: 9.99,
                    volume_remain: 100,
                    volume_total: 100,
                    issued: "2026-05-25T11:48:00Z".to_string(),
                    duration: 90,
                    min_volume: 1,
                    order_range: "station".to_string(),
                    system_id: 30000142,
                },
            ],
        )
        .await
        .unwrap();
    repository
        .complete_sync_run(sync_run_id, 1, 4)
        .await
        .unwrap();

    let books = repository
        .latest_station_order_books(10000002, 60003760, "en-US", 10)
        .await
        .unwrap();

    assert_eq!(books.len(), 1);
    assert_eq!(books[0].sync_run_id, sync_run_id);
    assert_eq!(books[0].type_id, 34);
    assert_eq!(books[0].display_name, "Type 34");
    assert_eq!(books[0].best_bid, 5.01);
    assert_eq!(books[0].best_ask, 5.49);
    assert_eq!(books[0].top_buy_depth, 625_000);
    assert_eq!(books[0].top_sell_depth, 650_000);
}
