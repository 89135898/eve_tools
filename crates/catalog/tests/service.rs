use evetools_catalog::{CatalogConfig, CatalogServiceError};

#[test]
fn config_requires_database_url() {
    let error = CatalogConfig::from_database_url("").unwrap_err();

    assert!(matches!(error, CatalogServiceError::MissingDatabaseUrl));
}
