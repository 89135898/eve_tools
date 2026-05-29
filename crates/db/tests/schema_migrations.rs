use evetools_db::{connect_pool, migrate_catalog_schema};
use evetools_test_support::{guarded_database_url_from_env, reset_evetools_catalog_schema};

static POSTGRES_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn records_versioned_sqlx_migrations() {
    let _guard = POSTGRES_TEST_LOCK.lock().await;
    let url = match guarded_database_url_from_env() {
        Ok(Some(url)) => url,
        Ok(None) => {
            eprintln!("skipping Postgres test: EVETOOLS_TEST_DATABASE_URL is not set");
            return;
        }
        Err(error) => panic!("{error}"),
    };
    let pool = connect_pool(&url).await.unwrap();
    reset_evetools_catalog_schema(&pool).await.unwrap();

    migrate_catalog_schema(&pool).await.unwrap();

    let versions: Vec<i64> = sqlx::query_scalar(
        "SELECT version
         FROM _sqlx_migrations
         ORDER BY version",
    )
    .persistent(false)
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(versions, vec![1, 2, 3, 4]);
}
