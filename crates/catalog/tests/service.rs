use evetools_catalog::{CatalogConfig, CatalogService, CatalogServiceError};

fn assert_clone<T: Clone>() {}

#[test]
fn config_requires_database_url() {
    let error = CatalogConfig::from_database_url("").unwrap_err();

    assert!(matches!(error, CatalogServiceError::MissingDatabaseUrl));
}

#[test]
fn config_rejects_supabase_transaction_pooler_for_catalog_imports() {
    let password = "catalog-password";
    let database_url = [
        "postgresql://postgres.example:",
        password,
        "@aws-1-ap-northeast-1.pooler.supabase.com:6543/postgres?sslmode=require",
    ]
    .concat();
    let error = CatalogConfig::from_database_url(database_url).unwrap_err();

    assert!(matches!(
        error,
        CatalogServiceError::UnsupportedTransactionPooler
    ));
    assert!(!error.to_string().contains(password));
}

#[test]
fn config_debug_redacts_database_url() {
    let database_url = "catalog-db-url-with-secret-password";
    let config = CatalogConfig::from_database_url(database_url).unwrap();

    let debug_output = format!("{config:?}");

    assert_eq!(
        debug_output,
        r#"CatalogConfig { database_url: "<redacted>" }"#
    );
    assert!(!debug_output.contains(database_url));
    assert!(!debug_output.contains("secret_password"));
}

#[test]
fn catalog_service_can_be_cloned_for_shared_state() {
    assert_clone::<CatalogService>();
}
