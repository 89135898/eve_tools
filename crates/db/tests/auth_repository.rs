use chrono::{Duration, Utc};
use evetools_db::{
    connect_pool, migrate_catalog_schema, AuthRepository, AuthorizedCharacter, CharacterAuthToken,
    CharacterOrderSnapshotInput,
};
use evetools_test_support::{guarded_database_url_from_env, reset_evetools_catalog_schema};

static POSTGRES_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn prepare_auth_repository() -> Option<AuthRepository> {
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
    Some(AuthRepository::new(pool))
}

fn authorized_character() -> AuthorizedCharacter {
    AuthorizedCharacter {
        character_id: 90_000_001,
        character_name: "Market Pilot".to_string(),
        owner_hash: Some("owner-hash".to_string()),
        last_login_at: "2026-05-29T10:00:00Z".to_string(),
    }
}

fn auth_token() -> CharacterAuthToken {
    CharacterAuthToken {
        character_id: 90_000_001,
        refresh_token: "refresh-token".to_string(),
        access_token: Some("access-token".to_string()),
        access_token_expires_at: Some((Utc::now() + Duration::minutes(20)).to_rfc3339()),
        scopes: vec!["esi-markets.read_character_orders.v1".to_string()],
        token_type: "Bearer".to_string(),
    }
}

fn sell_order(sync_run_id: i64) -> CharacterOrderSnapshotInput {
    CharacterOrderSnapshotInput {
        sync_run_id,
        character_id: 90_000_001,
        order_id: 8_000_000_001,
        type_id: 34,
        region_id: 10000002,
        location_id: 60003760,
        is_buy_order: false,
        price: 5.60,
        volume_remain: 100_000,
        volume_total: 200_000,
        issued: "2026-05-29T10:00:00Z".to_string(),
        duration: 90,
        min_volume: Some(1),
        order_range: "station".to_string(),
        is_corporation: false,
        escrow: Some(0.0),
    }
}

fn buy_order(sync_run_id: i64) -> CharacterOrderSnapshotInput {
    CharacterOrderSnapshotInput {
        sync_run_id,
        character_id: 90_000_001,
        order_id: 8_000_000_002,
        type_id: 34,
        region_id: 10000002,
        location_id: 60003760,
        is_buy_order: true,
        price: 4.95,
        volume_remain: 50_000,
        volume_total: 100_000,
        issued: "2026-05-29T10:05:00Z".to_string(),
        duration: 90,
        min_volume: Some(1),
        order_range: "station".to_string(),
        is_corporation: false,
        escrow: Some(120_000.0),
    }
}

#[tokio::test]
async fn stores_character_tokens_and_latest_order_snapshots() {
    let _guard = POSTGRES_TEST_LOCK.lock().await;
    let Some(repository) = prepare_auth_repository().await else {
        return;
    };

    repository
        .upsert_authorized_character(&authorized_character())
        .await
        .unwrap();
    repository.upsert_auth_token(&auth_token()).await.unwrap();

    let stored_token = repository.auth_token(90_000_001).await.unwrap().unwrap();
    assert_eq!(stored_token.character_id, 90_000_001);
    assert_eq!(stored_token.refresh_token, "refresh-token");
    assert_eq!(
        stored_token.scopes,
        vec!["esi-markets.read_character_orders.v1".to_string()]
    );
    let latest_character = repository
        .latest_authorized_character()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest_character.character_id, 90_000_001);
    assert_eq!(latest_character.character_name, "Market Pilot");

    let sync_run_id = repository
        .start_character_order_sync(90_000_001)
        .await
        .unwrap();
    repository
        .replace_character_order_snapshots(
            sync_run_id,
            &[sell_order(sync_run_id), buy_order(sync_run_id)],
        )
        .await
        .unwrap();
    repository
        .complete_character_order_sync(sync_run_id, 2)
        .await
        .unwrap();

    let orders = repository
        .latest_character_orders(90_000_001, 20)
        .await
        .unwrap();
    assert_eq!(orders.len(), 2);
    assert_eq!(orders[0].sync_run_id, sync_run_id);
    assert_eq!(orders[0].order_id, 8_000_000_001);
    assert!(!orders[0].is_buy_order);
    assert_eq!(orders[1].order_id, 8_000_000_002);
    assert!(orders[1].is_buy_order);
}

#[tokio::test]
async fn records_failed_character_order_sync_without_snapshots() {
    let _guard = POSTGRES_TEST_LOCK.lock().await;
    let Some(repository) = prepare_auth_repository().await else {
        return;
    };

    repository
        .upsert_authorized_character(&authorized_character())
        .await
        .unwrap();
    let sync_run_id = repository
        .start_character_order_sync(90_000_001)
        .await
        .unwrap();
    repository
        .fail_character_order_sync(sync_run_id, "esi unauthorized with access-token secret")
        .await
        .unwrap();

    let orders = repository
        .latest_character_orders(90_000_001, 20)
        .await
        .unwrap();
    assert!(orders.is_empty());

    let summary = repository
        .latest_character_order_sync(90_000_001)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(summary.sync_run_id, sync_run_id);
    assert_eq!(summary.status, "failed");
    assert_eq!(
        summary.error_summary.as_deref(),
        Some("esi unauthorized with [redacted] secret")
    );
}
