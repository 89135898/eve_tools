use evetools_db::{
    connect_pool, migrate_catalog_schema, CatalogImportProgress as DbCatalogImportProgress,
    CatalogRepository, ImportCatalogInput,
};
pub use evetools_db::{CatalogImportTable, CatalogStatus, InventoryTypeView};
use evetools_sde::{read_catalog_archive_from_bytes, SdeClient};
use std::fmt;
use thiserror::Error;
use url::Url;

const OFFICIAL_SDE_ARCHIVE_URL: &str =
    "https://developers.eveonline.com/static-data/eve-online-static-data-latest-jsonl.zip";
const MIN_COMPLETE_OFFICIAL_TYPE_COUNT: i64 = 10_000;
const MIN_COMPLETE_OFFICIAL_GROUP_COUNT: i64 = 500;
const MIN_COMPLETE_OFFICIAL_CATEGORY_COUNT: i64 = 20;
const MIN_COMPLETE_OFFICIAL_MARKET_GROUP_COUNT: i64 = 1_000;
const SUPABASE_TRANSACTION_POOLER_PORT: Option<u16> = Some(6543);

#[derive(Clone)]
/// Catalog database connection configuration.
///
/// Construct with [`CatalogConfig::from_database_url`] or [`CatalogConfig::from_env`].
///
/// ```compile_fail
/// use evetools_catalog::CatalogConfig;
///
/// let _config = CatalogConfig {
///     database_url: "catalog-db-url-with-fake-password".to_string(),
/// };
/// ```
pub struct CatalogConfig {
    database_url: String,
}

impl fmt::Debug for CatalogConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CatalogConfig")
            .field("database_url", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum CatalogServiceError {
    #[error("EVETOOLS_DATABASE_URL is required")]
    MissingDatabaseUrl,
    #[error(
        "EVETOOLS_DATABASE_URL uses the Supabase transaction pooler on port 6543; use the direct connection or session pooler for catalog imports"
    )]
    UnsupportedTransactionPooler,
    #[error("database error: {0}")]
    Database(#[from] evetools_db::CatalogDbError),
    #[error("sql migration or connection error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("SDE download error: {0}")]
    SdeClient(#[from] evetools_sde::SdeClientError),
    #[error("SDE archive error: {0}")]
    SdeArchive(#[from] evetools_sde::SdeArchiveError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogImportProgress {
    CheckingLatestMetadata,
    CheckingCurrentCatalog {
        latest_build_number: i32,
    },
    AlreadyCurrent {
        build_number: i32,
    },
    DownloadingArchive {
        url: String,
    },
    DownloadedArchive {
        byte_count: usize,
    },
    ParsingArchive,
    ParsedArchive {
        type_count: usize,
        group_count: usize,
        category_count: usize,
        market_group_count: usize,
    },
    WritingCatalog,
    WritingTableStarted {
        table: CatalogImportTable,
        total: usize,
    },
    WritingRows {
        table: CatalogImportTable,
        completed: usize,
        total: usize,
    },
    DeletingStaleRows,
    Completed {
        status: CatalogStatus,
    },
}

impl CatalogConfig {
    pub fn from_database_url(value: impl Into<String>) -> Result<Self, CatalogServiceError> {
        let database_url = value.into();
        if database_url.trim().is_empty() {
            return Err(CatalogServiceError::MissingDatabaseUrl);
        }
        reject_transaction_pooler(&database_url)?;
        Ok(Self { database_url })
    }

    pub fn from_env() -> Result<Self, CatalogServiceError> {
        Self::from_database_url(std::env::var("EVETOOLS_DATABASE_URL").unwrap_or_default())
    }
}

fn reject_transaction_pooler(database_url: &str) -> Result<(), CatalogServiceError> {
    let Ok(url) = Url::parse(database_url) else {
        return Ok(());
    };
    if url
        .host_str()
        .is_some_and(|host| host.ends_with(".pooler.supabase.com"))
        && url.port_or_known_default() == SUPABASE_TRANSACTION_POOLER_PORT
    {
        return Err(CatalogServiceError::UnsupportedTransactionPooler);
    }
    Ok(())
}

#[derive(Clone)]
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
        self.import_latest_with_progress(|_| {}).await
    }

    pub async fn import_latest_with_progress<F>(
        &self,
        mut progress: F,
    ) -> Result<CatalogStatus, CatalogServiceError>
    where
        F: FnMut(CatalogImportProgress),
    {
        let client = SdeClient::official()?;
        progress(CatalogImportProgress::CheckingLatestMetadata);
        let latest_metadata = client.latest_metadata().await?;
        progress(CatalogImportProgress::CheckingCurrentCatalog {
            latest_build_number: latest_metadata.build_number,
        });
        let current_status = self.status().await?;
        let has_localizations = self.repository.has_catalog_localizations().await?;
        if should_skip_latest_import(
            &current_status,
            latest_metadata.build_number,
            has_localizations,
        ) {
            progress(CatalogImportProgress::AlreadyCurrent {
                build_number: latest_metadata.build_number,
            });
            return Ok(current_status);
        }

        progress(CatalogImportProgress::DownloadingArchive {
            url: OFFICIAL_SDE_ARCHIVE_URL.to_string(),
        });
        let bytes = client.download_latest_archive().await?;
        progress(CatalogImportProgress::DownloadedArchive {
            byte_count: bytes.len(),
        });
        progress(CatalogImportProgress::ParsingArchive);
        let archive = read_catalog_archive_from_bytes(bytes)?;
        progress(CatalogImportProgress::ParsedArchive {
            type_count: archive.types.len(),
            group_count: archive.groups.len(),
            category_count: archive.categories.len(),
            market_group_count: archive.market_groups.len(),
        });
        progress(CatalogImportProgress::WritingCatalog);
        Ok(self
            .repository
            .import_archive_with_progress(
                ImportCatalogInput {
                    archive: &archive,
                    source_url: OFFICIAL_SDE_ARCHIVE_URL,
                },
                |event| match event {
                    DbCatalogImportProgress::TableStarted { table, total } => {
                        progress(CatalogImportProgress::WritingTableStarted { table, total });
                    }
                    DbCatalogImportProgress::TableAdvanced {
                        table,
                        completed,
                        total,
                    } => {
                        progress(CatalogImportProgress::WritingRows {
                            table,
                            completed,
                            total,
                        });
                    }
                    DbCatalogImportProgress::DeletingStaleRows => {
                        progress(CatalogImportProgress::DeletingStaleRows);
                    }
                },
            )
            .await
            .map(|status| {
                progress(CatalogImportProgress::Completed {
                    status: status.clone(),
                });
                status
            })?)
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
        Ok(self
            .repository
            .get_inventory_type(type_id, language)
            .await?)
    }
}

fn should_skip_latest_import(
    status: &CatalogStatus,
    latest_build_number: i32,
    has_localizations: bool,
) -> bool {
    status.status == "success"
        && has_localizations
        && status.build_number == Some(latest_build_number)
        && status.source_url.as_deref() == Some(OFFICIAL_SDE_ARCHIVE_URL)
        && status.type_count >= MIN_COMPLETE_OFFICIAL_TYPE_COUNT
        && status.group_count >= MIN_COMPLETE_OFFICIAL_GROUP_COUNT
        && status.category_count >= MIN_COMPLETE_OFFICIAL_CATEGORY_COUNT
        && status.market_group_count >= MIN_COMPLETE_OFFICIAL_MARKET_GROUP_COUNT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_skip_rejects_fixture_sized_status_even_when_build_matches() {
        let status = CatalogStatus {
            status: "success".to_string(),
            build_number: Some(3_351_823),
            release_date: Some("2026-05-19T12:12:31Z".to_string()),
            source_url: Some(OFFICIAL_SDE_ARCHIVE_URL.to_string()),
            completed_at: Some("2026-05-27T00:00:00Z".to_string()),
            error_summary: None,
            type_count: 1,
            group_count: 1,
            category_count: 1,
            market_group_count: 1,
        };

        assert!(!should_skip_latest_import(&status, 3_351_823, true));
    }

    #[test]
    fn import_skip_accepts_complete_official_status_when_build_matches() {
        let status = CatalogStatus {
            status: "success".to_string(),
            build_number: Some(3_351_823),
            release_date: Some("2026-05-19T12:12:31Z".to_string()),
            source_url: Some(OFFICIAL_SDE_ARCHIVE_URL.to_string()),
            completed_at: Some("2026-05-27T00:00:00Z".to_string()),
            error_summary: None,
            type_count: 10_000,
            group_count: 500,
            category_count: 20,
            market_group_count: 1_000,
        };

        assert!(should_skip_latest_import(&status, 3_351_823, true));
    }

    #[test]
    fn import_skip_rejects_complete_status_without_localization_rows() {
        let status = CatalogStatus {
            status: "success".to_string(),
            build_number: Some(3_351_823),
            release_date: Some("2026-05-19T12:12:31Z".to_string()),
            source_url: Some(OFFICIAL_SDE_ARCHIVE_URL.to_string()),
            completed_at: Some("2026-05-27T00:00:00Z".to_string()),
            error_summary: None,
            type_count: 10_000,
            group_count: 500,
            category_count: 20,
            market_group_count: 1_000,
        };

        assert!(!should_skip_latest_import(&status, 3_351_823, false));
    }
}
