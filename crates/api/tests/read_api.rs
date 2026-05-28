use evetools_api::{
    EveToolsReadApi, InventoryTypeLookupRequest, InventoryTypeSearchRequest,
    SelectionCandidatesRequest, StationOrdersRequest,
};
use evetools_db::{
    connect_pool, migrate_catalog_schema, CatalogRepository, ImportCatalogInput, MarketRepository,
    TradeHub,
};
use evetools_sde::{
    CatalogArchive, CatalogCategory, CatalogGroup, CatalogLocalization, CatalogMarketGroup,
    CatalogType, SdeMetadata,
};
use evetools_test_support::{guarded_database_url_from_env, reset_evetools_catalog_schema};

static POSTGRES_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn read_api_exposes_catalog_and_market_queries() {
    let _guard = POSTGRES_TEST_LOCK.lock().await;
    let Some(api) = prepare_seeded_api().await else {
        return;
    };

    let status = api.catalog_status().await.unwrap();
    assert_eq!(status.status, "success");
    assert_eq!(status.build_number, Some(3_351_823));

    let tritanium = api
        .get_inventory_type(InventoryTypeLookupRequest {
            type_id: 34,
            language: "zh".to_string(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(tritanium.display_name, "三钛合金");

    let search = api
        .search_inventory_types(InventoryTypeSearchRequest {
            query: "トリ".to_string(),
            language: "ja".to_string(),
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(search[0].display_name, "トリタニウム");

    let hubs = api.list_trade_hubs().await.unwrap();
    assert_eq!(hubs[0].hub_id, "jita");

    let orders = api
        .latest_station_orders(StationOrdersRequest {
            region_id: 10000002,
            station_id: 60003760,
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(orders.len(), 2);
    assert_eq!(orders[0].type_id, 34);

    let candidates = api
        .selection_candidates(SelectionCandidatesRequest {
            hub_ids: Vec::new(),
            language: "zh-CN".to_string(),
            limit_per_hub: 10,
        })
        .await
        .unwrap();
    assert_eq!(candidates.len(), 2);

    let jita = candidates
        .iter()
        .find(|candidate| candidate.hub_id == "jita")
        .unwrap();
    assert_eq!(jita.type_id, 34);
    assert_eq!(jita.item_name, "三钛合金");
    assert_eq!(jita.hub_name, "Jita");
    assert_eq!(jita.region_id, 10000002);
    assert_eq!(jita.station_id, 60003760);
    assert!(!jita.last_synced_at.is_empty());
    assert!(jita
        .reason_codes
        .iter()
        .any(|code| code == "healthy_spread"));

    let amarr = candidates
        .iter()
        .find(|candidate| candidate.hub_id == "amarr")
        .unwrap();
    assert_eq!(amarr.type_id, 34);
    assert_eq!(amarr.hub_name, "Amarr");
    assert_eq!(amarr.region_id, 10000043);
    assert_eq!(amarr.station_id, 60008494);

    let filtered = api
        .selection_candidates(SelectionCandidatesRequest {
            hub_ids: vec!["amarr".to_string()],
            language: "zh-CN".to_string(),
            limit_per_hub: 10,
        })
        .await
        .unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].hub_id, "amarr");
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
            source_url: "test://api-sample",
        })
        .await
        .unwrap();

    let market = MarketRepository::new(pool.clone());
    market
        .upsert_trade_hubs(&[jita_hub(), amarr_hub()])
        .await
        .unwrap();
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

    let amarr_sync_run_id = market.start_sync_run(10000043, "test").await.unwrap();
    market
        .replace_order_snapshots(
            amarr_sync_run_id,
            &[
                amarr_tritanium_buy_order(amarr_sync_run_id),
                amarr_tritanium_sell_order(amarr_sync_run_id),
            ],
        )
        .await
        .unwrap();
    market
        .complete_sync_run(amarr_sync_run_id, 1, 2)
        .await
        .unwrap();

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
                    language: "ja".to_string(),
                    name: Some("トリタニウム".to_string()),
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

fn amarr_tritanium_buy_order(sync_run_id: i64) -> evetools_db::MarketOrderSnapshotInput {
    evetools_db::MarketOrderSnapshotInput {
        sync_run_id,
        region_id: 10000043,
        station_id: 60008494,
        type_id: 34,
        order_id: 7_100_000_001,
        is_buy_order: true,
        price: 5.25,
        volume_remain: 300_000,
        volume_total: 300_000,
        issued: "2026-05-25T11:45:00Z".to_string(),
        duration: 90,
        min_volume: 1,
        order_range: "station".to_string(),
        system_id: 30002187,
    }
}

fn amarr_tritanium_sell_order(sync_run_id: i64) -> evetools_db::MarketOrderSnapshotInput {
    evetools_db::MarketOrderSnapshotInput {
        sync_run_id,
        region_id: 10000043,
        station_id: 60008494,
        type_id: 34,
        order_id: 7_100_000_002,
        is_buy_order: false,
        price: 5.99,
        volume_remain: 400_000,
        volume_total: 400_000,
        issued: "2026-05-25T11:46:00Z".to_string(),
        duration: 90,
        min_volume: 1,
        order_range: "station".to_string(),
        system_id: 30002187,
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
