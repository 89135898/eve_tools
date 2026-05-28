use evetools_api::EveToolsReadApi;
use evetools_db::{
    connect_pool, migrate_catalog_schema, CatalogRepository, ImportCatalogInput, MarketRepository,
    TradeHub,
};
use evetools_http_api::build_router;
use evetools_sde::{
    CatalogArchive, CatalogCategory, CatalogGroup, CatalogLocalization, CatalogMarketGroup,
    CatalogType, SdeMetadata,
};
use evetools_test_support::{guarded_database_url_from_env, reset_evetools_catalog_schema};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

static POSTGRES_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn read_http_api_exposes_health_hubs_and_selection_candidates() {
    let _guard = POSTGRES_TEST_LOCK.lock().await;
    let Some(api) = prepare_seeded_api().await else {
        return;
    };
    let router = build_router(api);

    let health = router.clone().oneshot(request("/health")).await.unwrap();
    assert_eq!(health.status(), 200);
    assert_eq!(json_body(health).await["status"], "ok");

    let hubs = router
        .clone()
        .oneshot(request("/trade-hubs"))
        .await
        .unwrap();
    assert_eq!(hubs.status(), 200);
    let hubs = json_body(hubs).await;
    assert_eq!(hubs.as_array().unwrap()[0]["hub_id"], "jita");

    let candidates = router
        .oneshot(request(
            "/selection-candidates?language=zh-CN&hub_ids=jita&limit_per_hub=10",
        ))
        .await
        .unwrap();
    assert_eq!(candidates.status(), 200);
    let candidates = json_body(candidates).await;
    assert_eq!(candidates.as_array().unwrap().len(), 1);
    assert_eq!(candidates.as_array().unwrap()[0]["hub_id"], "jita");
    assert_eq!(candidates.as_array().unwrap()[0]["item_name"], "三钛合金");
}

fn request(uri: &str) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .uri(uri)
        .body(axum::body::Body::empty())
        .unwrap()
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn prepare_seeded_api() -> Option<EveToolsReadApi> {
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

    let catalog = CatalogRepository::new(pool.clone());
    catalog
        .import_archive(ImportCatalogInput {
            archive: &sample_archive(),
            source_url: "test://http-api-sample",
        })
        .await
        .unwrap();

    let market = MarketRepository::new(pool.clone());
    market.upsert_trade_hubs(&[jita_hub()]).await.unwrap();
    let sync_run_id = market.start_sync_run(10000002, "test").await.unwrap();
    market
        .replace_order_snapshots(
            sync_run_id,
            &[
                tritanium_buy_order(sync_run_id),
                tritanium_sell_order(sync_run_id),
            ],
        )
        .await
        .unwrap();
    market.complete_sync_run(sync_run_id, 1, 2).await.unwrap();

    Some(EveToolsReadApi::from_pool(pool))
}

fn sample_archive() -> CatalogArchive {
    CatalogArchive {
        metadata: SdeMetadata {
            build_number: Some(3_351_823),
            release_date: Some("2026-05-19T12:12:31Z".to_string()),
        },
        types: vec![CatalogType {
            type_id: 34,
            group_id: 18,
            market_group_id: Some(1857),
            published: true,
            volume: Some(0.01),
            packaged_volume: Some(0.01),
            capacity: None,
            mass: Some(0.0),
            portion_size: Some(1),
            meta_level: None,
            name_en: Some("Tritanium".to_string()),
            name_zh: Some("三钛合金".to_string()),
            description_en: None,
            description_zh: None,
            raw_name_json: serde_json::json!({"en":"Tritanium","zh":"三钛合金"}),
            raw_description_json: None,
            localizations: vec![
                CatalogLocalization {
                    language: "en".to_string(),
                    name: Some("Tritanium".to_string()),
                    description: None,
                },
                CatalogLocalization {
                    language: "zh".to_string(),
                    name: Some("三钛合金".to_string()),
                    description: None,
                },
            ],
        }],
        groups: vec![CatalogGroup {
            group_id: 18,
            category_id: 4,
            published: true,
            name_en: Some("Mineral".to_string()),
            name_zh: Some("矿物".to_string()),
            raw_name_json: serde_json::json!({"en":"Mineral","zh":"矿物"}),
            localizations: vec![CatalogLocalization {
                language: "en".to_string(),
                name: Some("Mineral".to_string()),
                description: None,
            }],
        }],
        categories: vec![CatalogCategory {
            category_id: 4,
            published: true,
            name_en: Some("Material".to_string()),
            name_zh: Some("材料".to_string()),
            raw_name_json: serde_json::json!({"en":"Material","zh":"材料"}),
            localizations: vec![CatalogLocalization {
                language: "en".to_string(),
                name: Some("Material".to_string()),
                description: None,
            }],
        }],
        market_groups: vec![CatalogMarketGroup {
            market_group_id: 1857,
            parent_group_id: None,
            name_en: Some("Minerals".to_string()),
            name_zh: Some("矿物".to_string()),
            description_en: None,
            description_zh: None,
            raw_name_json: serde_json::json!({"en":"Minerals","zh":"矿物"}),
            raw_description_json: None,
            localizations: vec![CatalogLocalization {
                language: "en".to_string(),
                name: Some("Minerals".to_string()),
                description: None,
            }],
        }],
    }
}

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

fn tritanium_buy_order(sync_run_id: i64) -> evetools_db::MarketOrderSnapshotInput {
    evetools_db::MarketOrderSnapshotInput {
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

fn tritanium_sell_order(sync_run_id: i64) -> evetools_db::MarketOrderSnapshotInput {
    evetools_db::MarketOrderSnapshotInput {
        sync_run_id,
        region_id: 10000002,
        station_id: 60003760,
        type_id: 34,
        order_id: 7_000_000_002,
        is_buy_order: false,
        price: 5.49,
        volume_remain: 650_000,
        volume_total: 650_000,
        issued: "2026-05-25T11:46:00Z".to_string(),
        duration: 90,
        min_volume: 1,
        order_range: "station".to_string(),
        system_id: 30000142,
    }
}
