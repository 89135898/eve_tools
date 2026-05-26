use evetools_db::{connect_pool, migrate_catalog_schema, CatalogRepository, ImportCatalogInput};
use evetools_sde::{
    CatalogArchive, CatalogCategory, CatalogGroup, CatalogMarketGroup, CatalogType, SdeMetadata,
};

fn database_url() -> Option<String> {
    std::env::var("EVETOOLS_TEST_DATABASE_URL").ok()
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
        }],
        groups: vec![CatalogGroup {
            group_id: 18,
            category_id: 4,
            published: true,
            name_en: Some("Mineral".to_string()),
            name_zh: Some("矿物".to_string()),
            raw_name_json: serde_json::json!({"en":"Mineral","zh":"矿物"}),
        }],
        categories: vec![CatalogCategory {
            category_id: 4,
            published: true,
            name_en: Some("Material".to_string()),
            name_zh: Some("材料".to_string()),
            raw_name_json: serde_json::json!({"en":"Material","zh":"材料"}),
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
        }],
    }
}

#[tokio::test]
async fn imports_and_searches_catalog_rows() {
    let Some(url) = database_url() else {
        eprintln!("skipping Postgres test: EVETOOLS_TEST_DATABASE_URL is not set");
        return;
    };
    let pool = connect_pool(&url).await.unwrap();
    migrate_catalog_schema(&pool).await.unwrap();
    let repository = CatalogRepository::new(pool.clone());

    let status = repository
        .import_archive(ImportCatalogInput {
            archive: &sample_archive(),
            source_url: "test://sample",
        })
        .await
        .unwrap();
    let zh = repository
        .get_inventory_type(34, "zh")
        .await
        .unwrap()
        .unwrap();
    let search = repository
        .search_inventory_types("三钛", "zh", 10)
        .await
        .unwrap();

    assert_eq!(status.status, "success");
    assert_eq!(status.build_number, Some(3_351_823));
    assert_eq!(zh.display_name, "三钛合金");
    assert_eq!(search[0].type_id, 34);
}
