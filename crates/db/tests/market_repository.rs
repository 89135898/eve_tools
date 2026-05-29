use chrono::{Duration, Utc};
use evetools_db::{
    connect_pool, migrate_catalog_schema, MarketDbError, MarketOrderSnapshotInput,
    MarketRepository, MarketSyncHealthStatus, MarketSyncStartStatus, TradeHub,
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

fn amarr_hub() -> TradeHub {
    TradeHub {
        hub_id: "amarr".to_string(),
        display_name: "Amarr".to_string(),
        region_id: 10000043,
        system_id: 30002187,
        station_id: 60008494,
        enabled: true,
        sort_order: 20,
    }
}

fn dodixie_hub() -> TradeHub {
    TradeHub {
        hub_id: "dodixie".to_string(),
        display_name: "Dodixie".to_string(),
        region_id: 10000032,
        system_id: 30002659,
        station_id: 60011866,
        enabled: true,
        sort_order: 30,
    }
}

async fn prepare_market_repository() -> Option<(sqlx::PgPool, MarketRepository)> {
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
    let repository = MarketRepository::new(pool.clone());
    Some((pool, repository))
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

#[test]
fn market_sync_statuses_serialize_as_snake_case() {
    assert_eq!(
        serde_json::to_string(&MarketSyncHealthStatus::Fresh).unwrap(),
        "\"fresh\""
    );
    assert_eq!(
        serde_json::to_string(&MarketSyncStartStatus::AlreadyRunning).unwrap(),
        "\"already_running\""
    );
}

#[tokio::test]
async fn persists_trade_hubs_sync_runs_and_latest_station_orders() {
    let _guard = POSTGRES_TEST_LOCK.lock().await;
    let Some((_pool, repository)) = prepare_market_repository().await else {
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
    let Some((_pool, repository)) = prepare_market_repository().await else {
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

#[tokio::test]
async fn leases_reject_concurrent_region_syncs_and_expire_stale_runs() {
    let _guard = POSTGRES_TEST_LOCK.lock().await;
    let Some((pool, repository)) = prepare_market_repository().await else {
        return;
    };

    let lease = repository
        .try_start_sync_run(
            10000002,
            "public-esi",
            "test-worker",
            "lease-a",
            Duration::minutes(20),
        )
        .await
        .unwrap();
    assert_eq!(lease.status, MarketSyncStartStatus::Started);
    let sync_run_id = lease.sync_run_id.unwrap();

    let blocked = repository
        .try_start_sync_run(
            10000002,
            "public-esi",
            "test-worker",
            "lease-b",
            Duration::minutes(20),
        )
        .await
        .unwrap();
    assert_eq!(blocked.status, MarketSyncStartStatus::AlreadyRunning);
    assert_eq!(blocked.sync_run_id, Some(sync_run_id));

    sqlx::query(
        "UPDATE evetools_catalog.market_sync_runs
         SET lease_expires_at = NOW() - INTERVAL '1 minute'
         WHERE sync_run_id = $1",
    )
    .bind(sync_run_id)
    .execute(&pool)
    .await
    .unwrap();

    let replacement = repository
        .try_start_sync_run(
            10000002,
            "public-esi",
            "test-worker",
            "lease-c",
            Duration::minutes(20),
        )
        .await
        .unwrap();
    assert_eq!(replacement.status, MarketSyncStartStatus::Started);
    assert_ne!(replacement.sync_run_id, Some(sync_run_id));

    let expired_status: String = sqlx::query_scalar(
        "SELECT status FROM evetools_catalog.market_sync_runs WHERE sync_run_id = $1",
    )
    .bind(sync_run_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(expired_status, "expired");

    let replacement_id = replacement.sync_run_id.unwrap();
    repository
        .mark_sync_run_running(replacement_id)
        .await
        .unwrap();
    repository
        .complete_sync_run(replacement_id, 3, 42)
        .await
        .unwrap();
    let completion: (Option<i64>, Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT duration_ms, completed_reason, lease_owner, lease_expires_at::TEXT
             FROM evetools_catalog.market_sync_runs
             WHERE sync_run_id = $1",
    )
    .bind(replacement_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(completion.0.unwrap() >= 0);
    assert_eq!(completion.1.as_deref(), Some("completed"));
    assert_eq!(completion.2, None);
    assert_eq!(completion.3, None);
}

#[tokio::test]
async fn legacy_running_runs_without_lease_expire_after_twenty_minutes() {
    let _guard = POSTGRES_TEST_LOCK.lock().await;
    let Some((pool, repository)) = prepare_market_repository().await else {
        return;
    };

    let legacy_run_id = repository
        .start_sync_run(10000002, "public-esi")
        .await
        .unwrap();
    sqlx::query(
        "UPDATE evetools_catalog.market_sync_runs
         SET started_at = NOW() - INTERVAL '21 minutes'
         WHERE sync_run_id = $1",
    )
    .bind(legacy_run_id)
    .execute(&pool)
    .await
    .unwrap();

    let replacement = repository
        .try_start_sync_run(
            10000002,
            "public-esi",
            "test-worker",
            "lease-replacement",
            Duration::minutes(20),
        )
        .await
        .unwrap();
    assert_eq!(replacement.status, MarketSyncStartStatus::Started);

    let legacy: (String, Option<String>) = sqlx::query_as(
        "SELECT status, completed_reason
         FROM evetools_catalog.market_sync_runs
         WHERE sync_run_id = $1",
    )
    .bind(legacy_run_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(legacy.0, "expired");
    assert_eq!(legacy.1.as_deref(), Some("lease_expired"));
}

#[tokio::test]
async fn expired_leased_runs_cannot_publish_snapshots_or_complete_after_takeover() {
    let _guard = POSTGRES_TEST_LOCK.lock().await;
    let Some((pool, repository)) = prepare_market_repository().await else {
        return;
    };

    let stale = repository
        .try_start_sync_run(
            10000002,
            "public-esi",
            "test-worker",
            "lease-a",
            Duration::minutes(20),
        )
        .await
        .unwrap();
    let stale_run_id = stale.sync_run_id.unwrap();
    repository
        .mark_sync_run_running(stale_run_id)
        .await
        .unwrap();

    sqlx::query(
        "UPDATE evetools_catalog.market_sync_runs
         SET lease_expires_at = NOW() - INTERVAL '1 minute'
         WHERE sync_run_id = $1",
    )
    .bind(stale_run_id)
    .execute(&pool)
    .await
    .unwrap();

    let replacement = repository
        .try_start_sync_run(
            10000002,
            "public-esi",
            "test-worker",
            "lease-b",
            Duration::minutes(20),
        )
        .await
        .unwrap();
    let replacement_run_id = replacement.sync_run_id.unwrap();

    let stale_replace = repository
        .replace_order_snapshots(stale_run_id, &[tritanium_order(stale_run_id)])
        .await;
    assert!(matches!(
        stale_replace,
        Err(MarketDbError::InactiveSyncRun { sync_run_id }) if sync_run_id == stale_run_id
    ));

    let stale_complete = repository.complete_sync_run(stale_run_id, 1, 1).await;
    assert!(matches!(
        stale_complete,
        Err(MarketDbError::InactiveSyncRun { sync_run_id }) if sync_run_id == stale_run_id
    ));

    let stale_status: String = sqlx::query_scalar(
        "SELECT status FROM evetools_catalog.market_sync_runs WHERE sync_run_id = $1",
    )
    .bind(stale_run_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stale_status, "expired");

    repository
        .mark_sync_run_running(replacement_run_id)
        .await
        .unwrap();
    repository
        .replace_order_snapshots(replacement_run_id, &[tritanium_order(replacement_run_id)])
        .await
        .unwrap();
    repository
        .complete_sync_run(replacement_run_id, 1, 1)
        .await
        .unwrap();

    let replacement_status: String = sqlx::query_scalar(
        "SELECT status FROM evetools_catalog.market_sync_runs WHERE sync_run_id = $1",
    )
    .bind(replacement_run_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(replacement_status, "success");
}

#[tokio::test]
async fn sync_health_classifies_missing_fresh_stale_expired_syncing_and_degraded() {
    let _guard = POSTGRES_TEST_LOCK.lock().await;
    let Some((pool, repository)) = prepare_market_repository().await else {
        return;
    };

    repository.upsert_trade_hubs(&[jita_hub()]).await.unwrap();
    let now = Utc::now();

    let missing = repository.sync_health_at(now).await.unwrap();
    assert_eq!(missing.generated_at, now.to_rfc3339());
    assert_eq!(missing.hubs[0].status, MarketSyncHealthStatus::Missing);
    assert_eq!(missing.hubs[0].age_seconds, None);

    let fresh_run = repository
        .start_sync_run(10000002, "public-esi")
        .await
        .unwrap();
    repository.complete_sync_run(fresh_run, 1, 1).await.unwrap();
    let fresh = repository.sync_health_at(now).await.unwrap();
    assert_eq!(fresh.hubs[0].status, MarketSyncHealthStatus::Fresh);
    assert_eq!(fresh.hubs[0].latest_success_sync_run_id, Some(fresh_run));
    assert_eq!(fresh.hubs[0].order_count, Some(1));

    sqlx::query(
        "UPDATE evetools_catalog.market_sync_runs
         SET completed_at = NOW() - INTERVAL '20 minutes'
         WHERE sync_run_id = $1",
    )
    .bind(fresh_run)
    .execute(&pool)
    .await
    .unwrap();
    let stale = repository.sync_health_at(now).await.unwrap();
    assert_eq!(stale.hubs[0].status, MarketSyncHealthStatus::Stale);

    sqlx::query(
        "UPDATE evetools_catalog.market_sync_runs
         SET completed_at = NOW() - INTERVAL '2 hours'
         WHERE sync_run_id = $1",
    )
    .bind(fresh_run)
    .execute(&pool)
    .await
    .unwrap();
    let expired = repository.sync_health_at(now).await.unwrap();
    assert_eq!(expired.hubs[0].status, MarketSyncHealthStatus::Expired);

    let active = repository
        .try_start_sync_run(
            10000002,
            "public-esi",
            "test-worker",
            "lease-syncing",
            Duration::minutes(20),
        )
        .await
        .unwrap();
    let syncing = repository.sync_health_at(now).await.unwrap();
    assert_eq!(syncing.hubs[0].status, MarketSyncHealthStatus::Syncing);
    assert_eq!(
        syncing.hubs[0].latest_attempt_sync_run_id,
        active.sync_run_id
    );

    repository
        .fail_sync_run(active.sync_run_id.unwrap(), "esi unavailable")
        .await
        .unwrap();
    let degraded = repository.sync_health_at(now).await.unwrap();
    assert_eq!(degraded.hubs[0].status, MarketSyncHealthStatus::Degraded);
    assert_eq!(
        degraded.hubs[0].latest_attempt_status.as_deref(),
        Some("failed")
    );
    assert_eq!(
        degraded.hubs[0].latest_attempt_error.as_deref(),
        Some("esi unavailable")
    );
    assert_eq!(degraded.hubs[0].consecutive_failures, 1);

    let failure: (Option<i64>, Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT duration_ms, completed_reason, lease_owner, lease_expires_at::TEXT
         FROM evetools_catalog.market_sync_runs
         WHERE sync_run_id = $1",
    )
    .bind(active.sync_run_id.unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(failure.0.unwrap() >= 0);
    assert_eq!(failure.1.as_deref(), Some("failed"));
    assert_eq!(failure.2, None);
    assert_eq!(failure.3, None);
}

#[tokio::test]
async fn sync_health_uses_hub_specific_freshness_thresholds_in_sort_order() {
    let _guard = POSTGRES_TEST_LOCK.lock().await;
    let Some((pool, repository)) = prepare_market_repository().await else {
        return;
    };

    repository
        .upsert_trade_hubs(&[dodixie_hub(), jita_hub(), amarr_hub()])
        .await
        .unwrap();

    let jita_run = repository
        .start_sync_run(10000002, "public-esi")
        .await
        .unwrap();
    repository.complete_sync_run(jita_run, 1, 10).await.unwrap();
    let amarr_run = repository
        .start_sync_run(10000043, "public-esi")
        .await
        .unwrap();
    repository
        .complete_sync_run(amarr_run, 1, 20)
        .await
        .unwrap();
    let dodixie_run = repository
        .start_sync_run(10000032, "public-esi")
        .await
        .unwrap();
    repository
        .complete_sync_run(dodixie_run, 1, 30)
        .await
        .unwrap();

    sqlx::query(
        "UPDATE evetools_catalog.market_sync_runs
         SET completed_at = NOW() - INTERVAL '35 minutes'
         WHERE sync_run_id IN ($1, $2, $3)",
    )
    .bind(jita_run)
    .bind(amarr_run)
    .bind(dodixie_run)
    .execute(&pool)
    .await
    .unwrap();

    let health = repository.sync_health_at(Utc::now()).await.unwrap();
    let statuses: Vec<(&str, MarketSyncHealthStatus)> = health
        .hubs
        .iter()
        .map(|hub| (hub.hub_id.as_str(), hub.status.clone()))
        .collect();
    assert_eq!(
        statuses,
        vec![
            ("jita", MarketSyncHealthStatus::Expired),
            ("amarr", MarketSyncHealthStatus::Stale),
            ("dodixie", MarketSyncHealthStatus::Fresh),
        ]
    );
}
