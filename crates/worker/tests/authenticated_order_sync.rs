use chrono::{Duration, Utc};
use evetools_db::{
    connect_pool, migrate_catalog_schema, AuthRepository, AuthorizedCharacter, CharacterAuthToken,
};
use evetools_esi::EsiClient;
use evetools_test_support::{guarded_database_url_from_env, reset_evetools_catalog_schema};
use evetools_worker::sync_authenticated_character_orders;
use httpmock::prelude::*;

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

async fn seed_character(repository: &AuthRepository, access_token: &str) {
    repository
        .upsert_authorized_character(&AuthorizedCharacter {
            character_id: 90_000_001,
            character_name: "Market Pilot".to_string(),
            owner_hash: Some("owner-hash".to_string()),
            last_login_at: "2026-05-29T10:00:00Z".to_string(),
        })
        .await
        .unwrap();
    repository
        .upsert_auth_token(&CharacterAuthToken {
            character_id: 90_000_001,
            refresh_token: "refresh-token".to_string(),
            access_token: Some(access_token.to_string()),
            access_token_expires_at: Some((Utc::now() - Duration::minutes(1)).to_rfc3339()),
            scopes: vec!["esi-markets.read_character_orders.v1".to_string()],
            token_type: "Bearer".to_string(),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn refreshes_expired_token_and_syncs_character_orders() {
    let _guard = POSTGRES_TEST_LOCK.lock().await;
    let Some(repository) = prepare_auth_repository().await else {
        return;
    };
    seed_character(&repository, "expired-access-token").await;
    let server = MockServer::start();
    let refresh = server.mock(|when, then| {
        when.method(POST)
            .path("/v2/oauth/token")
            .body("grant_type=refresh_token&client_id=client-id&refresh_token=refresh-token");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{"access_token":"new-access-token","expires_in":1200,"token_type":"Bearer","refresh_token":"new-refresh-token"}"#,
            );
    });
    let orders = server.mock(|when, then| {
        when.method(GET)
            .path("/latest/characters/90000001/orders/")
            .query_param("datasource", "tranquility")
            .header("authorization", "Bearer new-access-token");
        then.status(200)
            .header("content-type", "application/json")
            .body(include_str!(
                "../../esi/tests/fixtures/character_orders.json"
            ));
    });
    let client = EsiClient::new(server.base_url());

    let summary = sync_authenticated_character_orders(
        &repository,
        &client,
        &server.base_url(),
        "client-id",
        90_000_001,
    )
    .await
    .unwrap();

    refresh.assert();
    orders.assert();
    assert_eq!(summary.character_id, 90_000_001);
    assert_eq!(summary.status, "success");
    assert_eq!(summary.order_count, 2);

    let stored_token = repository.auth_token(90_000_001).await.unwrap().unwrap();
    assert_eq!(
        stored_token.access_token.as_deref(),
        Some("new-access-token")
    );
    assert_eq!(stored_token.refresh_token, "new-refresh-token");

    let snapshots = repository
        .latest_character_orders(90_000_001, 20)
        .await
        .unwrap();
    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].order_id, 8_000_000_001);
    assert_eq!(snapshots[1].order_id, 8_000_000_002);
}

#[tokio::test]
async fn records_failed_character_order_sync_without_leaking_tokens() {
    let _guard = POSTGRES_TEST_LOCK.lock().await;
    let Some(repository) = prepare_auth_repository().await else {
        return;
    };
    seed_character(&repository, "access-token").await;
    repository
        .upsert_auth_token(&CharacterAuthToken {
            character_id: 90_000_001,
            refresh_token: "refresh-token".to_string(),
            access_token: Some("access-token".to_string()),
            access_token_expires_at: Some((Utc::now() + Duration::minutes(20)).to_rfc3339()),
            scopes: vec!["esi-markets.read_character_orders.v1".to_string()],
            token_type: "Bearer".to_string(),
        })
        .await
        .unwrap();
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/latest/characters/90000001/orders/")
            .query_param("datasource", "tranquility")
            .header("authorization", "Bearer access-token");
        then.status(401)
            .header("content-type", "text/plain")
            .body("access-token rejected");
    });
    let client = EsiClient::new(server.base_url());

    let error = sync_authenticated_character_orders(
        &repository,
        &client,
        &server.base_url(),
        "client-id",
        90_000_001,
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("ESI returned status 401"));
    let latest = repository
        .latest_character_order_sync(90_000_001)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, "failed");
    assert_eq!(
        latest.error_summary.as_deref(),
        Some("ESI returned status 401: [redacted] rejected")
    );
}
