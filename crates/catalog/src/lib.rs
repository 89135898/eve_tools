use evetools_db::{
    connect_pool, migrate_catalog_schema, CatalogRepository, CatalogStatus, ImportCatalogInput,
    InventoryTypeView,
};
use evetools_sde::{read_catalog_archive_from_bytes, SdeClient};
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct CatalogConfig {
    pub database_url: String,
}

#[derive(Debug, Error)]
pub enum CatalogServiceError {
    #[error("EVETOOLS_DATABASE_URL is required")]
    MissingDatabaseUrl,
    #[error("database error: {0}")]
    Database(#[from] evetools_db::CatalogDbError),
    #[error("sql migration or connection error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("SDE download error: {0}")]
    SdeClient(#[from] evetools_sde::SdeClientError),
    #[error("SDE archive error: {0}")]
    SdeArchive(#[from] evetools_sde::SdeArchiveError),
}

impl CatalogConfig {
    pub fn from_database_url(value: impl Into<String>) -> Result<Self, CatalogServiceError> {
        let database_url = value.into();
        if database_url.trim().is_empty() {
            return Err(CatalogServiceError::MissingDatabaseUrl);
        }
        Ok(Self { database_url })
    }

    pub fn from_env() -> Result<Self, CatalogServiceError> {
        Self::from_database_url(std::env::var("EVETOOLS_DATABASE_URL").unwrap_or_default())
    }
}

pub struct CatalogService {
    repository: CatalogRepository,
}

impl CatalogService {
    pub async fn connect(config: CatalogConfig) -> Result<Self, CatalogServiceError> {
        let pool = connect_pool(&config.database_url).await?;
        migrate_catalog_schema(&pool).await?;
        Ok(Self {
            repository: CatalogRepository::new(pool),
        })
    }

    pub async fn status(&self) -> Result<CatalogStatus, CatalogServiceError> {
        Ok(self.repository.latest_status().await?)
    }

    pub async fn import_latest(&self) -> Result<CatalogStatus, CatalogServiceError> {
        let client = SdeClient::official()?;
        let bytes = client.download_latest_archive().await?;
        let archive = read_catalog_archive_from_bytes(bytes)?;
        Ok(self
            .repository
            .import_archive(ImportCatalogInput {
                archive: &archive,
                source_url: "https://developers.eveonline.com/static-data/eve-online-static-data-latest-jsonl.zip",
            })
            .await?)
    }

    pub async fn search_inventory_types(
        &self,
        query: &str,
        language: &str,
        limit: i64,
    ) -> Result<Vec<InventoryTypeView>, CatalogServiceError> {
        Ok(self
            .repository
            .search_inventory_types(query, language, limit)
            .await?)
    }

    pub async fn get_inventory_type(
        &self,
        type_id: i32,
        language: &str,
    ) -> Result<Option<InventoryTypeView>, CatalogServiceError> {
        Ok(self.repository.get_inventory_type(type_id, language).await?)
    }
}
