use chrono::Utc;
use evetools_sde::{
    CatalogArchive, CatalogCategory, CatalogGroup, CatalogLocalization, CatalogMarketGroup,
    CatalogType,
};
use serde::{Deserialize, Serialize};
use sqlx::{pool::PoolConnection, postgres::PgRow, PgPool, Postgres, Row, Transaction};
use std::collections::HashSet;
use thiserror::Error;

const CATALOG_IMPORT_LOCK_KEY: i64 = 912_345_678_901_234_567;
const MAX_SEARCH_LIMIT: i64 = 100;
const TABLE_PROGRESS_REPORT_INTERVAL: usize = 1_000;
const IMPORT_TABLE_MAX_ATTEMPTS: usize = 3;
#[cfg(test)]
const IMPORT_BATCH_EXECUTION_SCOPE: &str = "copy-staging";
const IMPORT_TRANSACTION_SETUP_STATEMENTS: &[&str] = &[
    "SET LOCAL lock_timeout = '15s'",
    "SET LOCAL idle_in_transaction_session_timeout = '60s'",
    "SET LOCAL statement_timeout = '120s'",
];

#[derive(Debug, Error)]
pub enum CatalogDbError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogStatus {
    pub status: String,
    pub build_number: Option<i32>,
    pub release_date: Option<String>,
    pub source_url: Option<String>,
    pub completed_at: Option<String>,
    pub error_summary: Option<String>,
    pub type_count: i64,
    pub group_count: i64,
    pub category_count: i64,
    pub market_group_count: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryTypeView {
    pub type_id: i32,
    pub group_id: i32,
    pub category_id: Option<i32>,
    pub market_group_id: Option<i32>,
    pub display_name: String,
    pub name_en: Option<String>,
    pub name_zh: Option<String>,
    pub group_name: Option<String>,
    pub category_name: Option<String>,
    pub market_group_name: Option<String>,
    pub published: bool,
    pub market_eligible: bool,
}

pub struct ImportCatalogInput<'a> {
    pub archive: &'a CatalogArchive,
    pub source_url: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogImportTable {
    Categories,
    CategoryLocalizations,
    Groups,
    GroupLocalizations,
    MarketGroups,
    MarketGroupLocalizations,
    Types,
    TypeLocalizations,
}

impl CatalogImportTable {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Categories => "categories",
            Self::CategoryLocalizations => "category localizations",
            Self::Groups => "groups",
            Self::GroupLocalizations => "group localizations",
            Self::MarketGroups => "market groups",
            Self::MarketGroupLocalizations => "market group localizations",
            Self::Types => "types",
            Self::TypeLocalizations => "type localizations",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogImportProgress {
    TableStarted {
        table: CatalogImportTable,
        total: usize,
    },
    BatchStarted {
        table: CatalogImportTable,
        completed: usize,
        total: usize,
        batch_size: usize,
        attempt: usize,
    },
    BatchMerging {
        table: CatalogImportTable,
        completed: usize,
        total: usize,
        batch_size: usize,
        attempt: usize,
    },
    BatchRetrying {
        table: CatalogImportTable,
        completed: usize,
        total: usize,
        batch_size: usize,
        next_attempt: usize,
        error_summary: String,
    },
    TableAdvanced {
        table: CatalogImportTable,
        completed: usize,
        total: usize,
    },
    DeletingStaleRows,
}

#[derive(Clone)]
pub struct CatalogRepository {
    pool: PgPool,
}

impl CatalogRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn latest_status(&self) -> Result<CatalogStatus, CatalogDbError> {
        let row = sqlx::query_as::<_, CatalogStatusRecord>(
            "SELECT status, build_number, release_date, source_url, completed_at, error_summary,
                    type_count, group_count, category_count, market_group_count
             FROM evetools_catalog.sde_imports
             ORDER BY import_id DESC
             LIMIT 1",
        )
        .persistent(false)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row
            .map(catalog_status_from_record)
            .unwrap_or_else(not_imported_status))
    }

    pub async fn has_catalog_localizations(&self) -> Result<bool, CatalogDbError> {
        let has_localizations: bool = sqlx::query_scalar(
            "SELECT
                EXISTS (
                SELECT 1
                FROM evetools_catalog.inventory_type_localizations
                LIMIT 1
                )
                AND EXISTS (
                    SELECT 1
                    FROM evetools_catalog.inventory_group_localizations
                    LIMIT 1
                )
                AND EXISTS (
                    SELECT 1
                    FROM evetools_catalog.inventory_category_localizations
                    LIMIT 1
                )
                AND EXISTS (
                    SELECT 1
                    FROM evetools_catalog.market_group_localizations
                    LIMIT 1
                )",
        )
        .persistent(false)
        .fetch_one(&self.pool)
        .await?;

        Ok(has_localizations)
    }

    pub async fn import_archive(
        &self,
        input: ImportCatalogInput<'_>,
    ) -> Result<CatalogStatus, CatalogDbError> {
        self.import_archive_with_progress(input, |_| {}).await
    }

    pub async fn import_archive_with_progress<F>(
        &self,
        input: ImportCatalogInput<'_>,
        mut progress: F,
    ) -> Result<CatalogStatus, CatalogDbError>
    where
        F: FnMut(CatalogImportProgress),
    {
        let mut lock = acquire_import_lock(&self.pool).await?;
        let result = self.import_archive_with_lock(input, &mut progress).await;
        let unlock_result = release_import_lock(&mut lock).await;

        match (result, unlock_result) {
            (Ok(status), Ok(())) => Ok(status),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error.into()),
        }
    }

    async fn import_archive_with_lock<F>(
        &self,
        input: ImportCatalogInput<'_>,
        progress: &mut F,
    ) -> Result<CatalogStatus, CatalogDbError>
    where
        F: FnMut(CatalogImportProgress),
    {
        let mut tx = begin_import_transaction(&self.pool).await?;
        if let Some(build_number) = input.archive.metadata.build_number {
            if let Some(successful_import) =
                latest_success_status_for_build(&mut tx, build_number).await?
            {
                if successful_import_matches_input(&successful_import.status, &input)
                    && localization_counts_match_input(&mut tx, successful_import.import_id, &input)
                        .await?
                {
                    tx.commit().await?;
                    return Ok(successful_import.status);
                }
            }
        }

        let import_id: i64 = sqlx::query_scalar(
            "INSERT INTO evetools_catalog.sde_imports
                (build_number, release_date, source_url, started_at, status)
             VALUES ($1, $2, $3, NOW(), 'running')
             RETURNING import_id",
        )
        .persistent(false)
        .bind(input.archive.metadata.build_number)
        .bind(input.archive.metadata.release_date.as_deref())
        .bind(input.source_url)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;

        let result = async {
            import_categories(&self.pool, import_id, input.archive, progress).await?;
            import_category_localizations(&self.pool, import_id, input.archive, progress).await?;
            import_groups(&self.pool, import_id, input.archive, progress).await?;
            import_group_localizations(&self.pool, import_id, input.archive, progress).await?;
            import_market_groups(&self.pool, import_id, input.archive, progress).await?;
            import_market_group_localizations(&self.pool, import_id, input.archive, progress)
                .await?;
            import_types(&self.pool, import_id, input.archive, progress).await?;
            import_type_localizations(&self.pool, import_id, input.archive, progress).await?;
            progress(CatalogImportProgress::DeletingStaleRows);
            import_delete_stale_catalog_rows(&self.pool, import_id).await?;
            Ok::<(), sqlx::Error>(())
        }
        .await;
        if let Err(error) = result {
            let _ = mark_import_failed(&self.pool, import_id, &error.to_string()).await;
            return Err(error.into());
        }

        sqlx::query(
            "UPDATE evetools_catalog.sde_imports
             SET completed_at = NOW(), status = 'success',
                 type_count = $1, group_count = $2,
                 category_count = $3, market_group_count = $4
             WHERE import_id = $5",
        )
        .persistent(false)
        .bind(input.archive.types.len() as i64)
        .bind(input.archive.groups.len() as i64)
        .bind(input.archive.categories.len() as i64)
        .bind(input.archive.market_groups.len() as i64)
        .bind(import_id)
        .execute(&self.pool)
        .await?;
        self.status_by_import_id(import_id).await
    }

    pub async fn get_inventory_type(
        &self,
        type_id: i32,
        language: &str,
    ) -> Result<Option<InventoryTypeView>, CatalogDbError> {
        let language_fallbacks = language_fallbacks(language);
        let row = sqlx::query_as::<_, InventoryTypeRow>(TYPE_SELECT_SQL)
            .persistent(false)
            .bind(type_id)
            .bind(language_fallbacks)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| row.into_view(language)))
    }

    pub async fn search_inventory_types(
        &self,
        query: &str,
        language: &str,
        limit: i64,
    ) -> Result<Vec<InventoryTypeView>, CatalogDbError> {
        let Some(limit) = search_limit(limit) else {
            return Ok(Vec::new());
        };
        let Some(pattern) = search_pattern(query) else {
            return Ok(Vec::new());
        };
        let language_fallbacks = language_fallbacks(language);
        let rows = sqlx::query_as::<_, InventoryTypeRow>(
            "SELECT t.type_id, t.group_id, g.category_id, t.market_group_id,
                    COALESCE(
                        (SELECT l.name
                         FROM evetools_catalog.inventory_type_localizations l
                         WHERE l.type_id = t.type_id AND l.name IS NOT NULL
                         ORDER BY COALESCE(array_position($2::text[], l.language), 2147483647), l.language
                         LIMIT 1),
                        t.name_en, t.name_zh
                    ) AS display_name,
                    t.name_en, t.name_zh, g.name_en AS group_name_en, g.name_zh AS group_name_zh,
                    c.name_en AS category_name_en, c.name_zh AS category_name_zh,
                    mg.name_en AS market_group_name_en, mg.name_zh AS market_group_name_zh,
                    COALESCE(
                        (SELECT l.name
                         FROM evetools_catalog.inventory_group_localizations l
                         WHERE l.group_id = g.group_id AND l.name IS NOT NULL
                         ORDER BY COALESCE(array_position($2::text[], l.language), 2147483647), l.language
                         LIMIT 1),
                        g.name_en, g.name_zh
                    ) AS group_display_name,
                    COALESCE(
                        (SELECT l.name
                         FROM evetools_catalog.inventory_category_localizations l
                         WHERE l.category_id = c.category_id AND l.name IS NOT NULL
                         ORDER BY COALESCE(array_position($2::text[], l.language), 2147483647), l.language
                         LIMIT 1),
                        c.name_en, c.name_zh
                    ) AS category_display_name,
                    COALESCE(
                        (SELECT l.name
                         FROM evetools_catalog.market_group_localizations l
                         WHERE l.market_group_id = mg.market_group_id AND l.name IS NOT NULL
                         ORDER BY COALESCE(array_position($2::text[], l.language), 2147483647), l.language
                         LIMIT 1),
                        mg.name_en, mg.name_zh
                    ) AS market_group_display_name,
                    t.published,
                    (t.published AND t.market_group_id IS NOT NULL AND (t.name_en IS NOT NULL OR t.name_zh IS NOT NULL)) AS market_eligible
             FROM evetools_catalog.inventory_types t
             LEFT JOIN evetools_catalog.inventory_groups g ON g.group_id = t.group_id
             LEFT JOIN evetools_catalog.inventory_categories c ON c.category_id = g.category_id
             LEFT JOIN evetools_catalog.market_groups mg ON mg.market_group_id = t.market_group_id
             WHERE EXISTS (
                SELECT 1
                FROM evetools_catalog.inventory_type_localizations search_l
                WHERE search_l.type_id = t.type_id AND search_l.name ILIKE $1 ESCAPE '\\'
             ) OR t.name_en ILIKE $1 ESCAPE '\\' OR t.name_zh ILIKE $1 ESCAPE '\\'
             ORDER BY display_name NULLS LAST
             LIMIT $3",
        )
        .persistent(false)
        .bind(pattern)
        .bind(language_fallbacks)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| row.into_view(language))
            .collect())
    }

    async fn status_by_import_id(&self, import_id: i64) -> Result<CatalogStatus, CatalogDbError> {
        let row = sqlx::query_as::<_, CatalogStatusRecord>(
            "SELECT status, build_number, release_date, source_url, completed_at, error_summary,
                    type_count, group_count, category_count, market_group_count
             FROM evetools_catalog.sde_imports
             WHERE import_id = $1",
        )
        .persistent(false)
        .bind(import_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(catalog_status_from_record(row))
    }
}

type CatalogStatusRecord = (
    String,
    Option<i32>,
    Option<String>,
    Option<String>,
    Option<chrono::DateTime<Utc>>,
    Option<String>,
    i64,
    i64,
    i64,
    i64,
);

struct SuccessfulImportStatus {
    import_id: i64,
    status: CatalogStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CatalogLocalizationCounts {
    type_count: i64,
    group_count: i64,
    category_count: i64,
    market_group_count: i64,
}

fn catalog_status_from_record(row: CatalogStatusRecord) -> CatalogStatus {
    CatalogStatus {
        status: row.0,
        build_number: row.1,
        release_date: row.2,
        source_url: row.3,
        completed_at: row.4.map(|value| value.to_rfc3339()),
        error_summary: row.5,
        type_count: row.6,
        group_count: row.7,
        category_count: row.8,
        market_group_count: row.9,
    }
}

fn not_imported_status() -> CatalogStatus {
    CatalogStatus {
        status: "not-imported".to_string(),
        build_number: None,
        release_date: None,
        source_url: None,
        completed_at: None,
        error_summary: None,
        type_count: 0,
        group_count: 0,
        category_count: 0,
        market_group_count: 0,
    }
}

async fn latest_success_status_for_build(
    tx: &mut Transaction<'_, Postgres>,
    build_number: i32,
) -> Result<Option<SuccessfulImportStatus>, sqlx::Error> {
    let row = sqlx::query_as::<_, (i64, String, Option<i32>, Option<String>, Option<String>, Option<chrono::DateTime<Utc>>, Option<String>, i64, i64, i64, i64)>(
        "SELECT import_id, status, build_number, release_date, source_url, completed_at, error_summary,
                type_count, group_count, category_count, market_group_count
         FROM evetools_catalog.sde_imports
         WHERE status = 'success' AND build_number = $1
         ORDER BY import_id DESC
         LIMIT 1",
    )
    .persistent(false)
    .bind(build_number)
    .fetch_optional(&mut **tx)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };
    let import_id = row.0;
    let status = catalog_status_from_record((
        row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8, row.9, row.10,
    ));
    Ok(Some(SuccessfulImportStatus { import_id, status }))
}

fn successful_import_matches_input(status: &CatalogStatus, input: &ImportCatalogInput<'_>) -> bool {
    status.status == "success"
        && status.build_number == input.archive.metadata.build_number
        && status.source_url.as_deref() == Some(input.source_url)
        && status.type_count == input.archive.types.len() as i64
        && status.group_count == input.archive.groups.len() as i64
        && status.category_count == input.archive.categories.len() as i64
        && status.market_group_count == input.archive.market_groups.len() as i64
}

fn expected_localization_counts(archive: &CatalogArchive) -> CatalogLocalizationCounts {
    CatalogLocalizationCounts {
        type_count: archive
            .types
            .iter()
            .map(|row| row.localizations.len() as i64)
            .sum(),
        group_count: archive
            .groups
            .iter()
            .map(|row| row.localizations.len() as i64)
            .sum(),
        category_count: archive
            .categories
            .iter()
            .map(|row| row.localizations.len() as i64)
            .sum(),
        market_group_count: archive
            .market_groups
            .iter()
            .map(|row| row.localizations.len() as i64)
            .sum(),
    }
}

async fn localization_counts_for_import(
    tx: &mut Transaction<'_, Postgres>,
    import_id: i64,
) -> Result<CatalogLocalizationCounts, sqlx::Error> {
    let row = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT
            (SELECT COUNT(*) FROM evetools_catalog.inventory_type_localizations WHERE updated_import_id = $1),
            (SELECT COUNT(*) FROM evetools_catalog.inventory_group_localizations WHERE updated_import_id = $1),
            (SELECT COUNT(*) FROM evetools_catalog.inventory_category_localizations WHERE updated_import_id = $1),
            (SELECT COUNT(*) FROM evetools_catalog.market_group_localizations WHERE updated_import_id = $1)",
    )
    .persistent(false)
    .bind(import_id)
    .fetch_one(&mut **tx)
    .await?;

    Ok(CatalogLocalizationCounts {
        type_count: row.0,
        group_count: row.1,
        category_count: row.2,
        market_group_count: row.3,
    })
}

async fn localization_counts_match_input(
    tx: &mut Transaction<'_, Postgres>,
    import_id: i64,
    input: &ImportCatalogInput<'_>,
) -> Result<bool, sqlx::Error> {
    let actual = localization_counts_for_import(tx, import_id).await?;
    Ok(actual == expected_localization_counts(input.archive))
}

async fn acquire_import_lock(pool: &PgPool) -> Result<PoolConnection<Postgres>, sqlx::Error> {
    let mut connection = pool.acquire().await?;
    sqlx::query("SELECT pg_advisory_lock($1)")
        .persistent(false)
        .bind(CATALOG_IMPORT_LOCK_KEY)
        .execute(&mut *connection)
        .await?;
    Ok(connection)
}

async fn release_import_lock(connection: &mut PoolConnection<Postgres>) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .persistent(false)
        .bind(CATALOG_IMPORT_LOCK_KEY)
        .execute(&mut **connection)
        .await?;
    Ok(())
}

async fn begin_import_transaction(pool: &PgPool) -> Result<Transaction<'_, Postgres>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    for statement in IMPORT_TRANSACTION_SETUP_STATEMENTS {
        sqlx::query(statement)
            .persistent(false)
            .execute(&mut *tx)
            .await?;
    }
    Ok(tx)
}

const TYPE_SELECT_SQL: &str = "SELECT t.type_id, t.group_id, g.category_id, t.market_group_id,
        COALESCE(
            (SELECT l.name
             FROM evetools_catalog.inventory_type_localizations l
             WHERE l.type_id = t.type_id AND l.name IS NOT NULL
             ORDER BY COALESCE(array_position($2::text[], l.language), 2147483647), l.language
             LIMIT 1),
            t.name_en, t.name_zh
        ) AS display_name,
        t.name_en, t.name_zh, g.name_en AS group_name_en, g.name_zh AS group_name_zh,
        c.name_en AS category_name_en, c.name_zh AS category_name_zh,
        mg.name_en AS market_group_name_en, mg.name_zh AS market_group_name_zh,
        COALESCE(
            (SELECT l.name
             FROM evetools_catalog.inventory_group_localizations l
             WHERE l.group_id = g.group_id AND l.name IS NOT NULL
             ORDER BY COALESCE(array_position($2::text[], l.language), 2147483647), l.language
             LIMIT 1),
            g.name_en, g.name_zh
        ) AS group_display_name,
        COALESCE(
            (SELECT l.name
             FROM evetools_catalog.inventory_category_localizations l
             WHERE l.category_id = c.category_id AND l.name IS NOT NULL
             ORDER BY COALESCE(array_position($2::text[], l.language), 2147483647), l.language
             LIMIT 1),
            c.name_en, c.name_zh
        ) AS category_display_name,
        COALESCE(
            (SELECT l.name
             FROM evetools_catalog.market_group_localizations l
             WHERE l.market_group_id = mg.market_group_id AND l.name IS NOT NULL
             ORDER BY COALESCE(array_position($2::text[], l.language), 2147483647), l.language
             LIMIT 1),
            mg.name_en, mg.name_zh
        ) AS market_group_display_name,
        t.published,
        (t.published AND t.market_group_id IS NOT NULL AND (t.name_en IS NOT NULL OR t.name_zh IS NOT NULL)) AS market_eligible
    FROM evetools_catalog.inventory_types t
    LEFT JOIN evetools_catalog.inventory_groups g ON g.group_id = t.group_id
    LEFT JOIN evetools_catalog.inventory_categories c ON c.category_id = g.category_id
    LEFT JOIN evetools_catalog.market_groups mg ON mg.market_group_id = t.market_group_id
    WHERE t.type_id = $1";

struct InventoryTypeRow {
    type_id: i32,
    group_id: i32,
    category_id: Option<i32>,
    market_group_id: Option<i32>,
    display_name: Option<String>,
    name_en: Option<String>,
    name_zh: Option<String>,
    group_name_en: Option<String>,
    group_name_zh: Option<String>,
    category_name_en: Option<String>,
    category_name_zh: Option<String>,
    market_group_name_en: Option<String>,
    market_group_name_zh: Option<String>,
    group_display_name: Option<String>,
    category_display_name: Option<String>,
    market_group_display_name: Option<String>,
    published: bool,
    market_eligible: bool,
}

impl<'r> sqlx::FromRow<'r, PgRow> for InventoryTypeRow {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            type_id: row.try_get("type_id")?,
            group_id: row.try_get("group_id")?,
            category_id: row.try_get("category_id")?,
            market_group_id: row.try_get("market_group_id")?,
            display_name: row.try_get("display_name")?,
            name_en: row.try_get("name_en")?,
            name_zh: row.try_get("name_zh")?,
            group_name_en: row.try_get("group_name_en")?,
            group_name_zh: row.try_get("group_name_zh")?,
            category_name_en: row.try_get("category_name_en")?,
            category_name_zh: row.try_get("category_name_zh")?,
            market_group_name_en: row.try_get("market_group_name_en")?,
            market_group_name_zh: row.try_get("market_group_name_zh")?,
            group_display_name: row.try_get("group_display_name")?,
            category_display_name: row.try_get("category_display_name")?,
            market_group_display_name: row.try_get("market_group_display_name")?,
            published: row.try_get("published")?,
            market_eligible: row.try_get("market_eligible")?,
        })
    }
}

impl InventoryTypeRow {
    fn into_view(self, language: &str) -> InventoryTypeView {
        let prefer_zh = language.starts_with("zh");
        let display_name = self
            .display_name
            .clone()
            .or_else(|| choose_name(prefer_zh, self.name_zh.as_ref(), self.name_en.as_ref()))
            .unwrap_or_else(|| format!("Type {}", self.type_id));
        InventoryTypeView {
            type_id: self.type_id,
            group_id: self.group_id,
            category_id: self.category_id,
            market_group_id: self.market_group_id,
            display_name,
            name_en: self.name_en,
            name_zh: self.name_zh,
            group_name: self.group_display_name.or_else(|| {
                choose_name(
                    prefer_zh,
                    self.group_name_zh.as_ref(),
                    self.group_name_en.as_ref(),
                )
            }),
            category_name: self.category_display_name.or_else(|| {
                choose_name(
                    prefer_zh,
                    self.category_name_zh.as_ref(),
                    self.category_name_en.as_ref(),
                )
            }),
            market_group_name: self.market_group_display_name.or_else(|| {
                choose_name(
                    prefer_zh,
                    self.market_group_name_zh.as_ref(),
                    self.market_group_name_en.as_ref(),
                )
            }),
            published: self.published,
            market_eligible: self.market_eligible,
        }
    }
}

fn choose_name(prefer_zh: bool, zh: Option<&String>, en: Option<&String>) -> Option<String> {
    if prefer_zh {
        zh.or(en).cloned()
    } else {
        en.or(zh).cloned()
    }
}

fn search_limit(limit: i64) -> Option<i64> {
    if limit <= 0 {
        None
    } else {
        Some(limit.min(MAX_SEARCH_LIMIT))
    }
}

fn search_pattern(query: &str) -> Option<String> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }

    let mut pattern = String::with_capacity(query.len() + 2);
    pattern.push('%');
    for character in query.chars() {
        if matches!(character, '\\' | '%' | '_') {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    pattern.push('%');
    Some(pattern)
}

fn language_fallbacks(language: &str) -> Vec<String> {
    let normalized = language.trim().replace('_', "-");
    let mut fallbacks = Vec::new();
    push_unique_language(&mut fallbacks, normalized.as_str());

    if let Some((base, _)) = normalized.split_once('-') {
        push_unique_language(&mut fallbacks, base);
    }
    if normalized.starts_with("zh") {
        push_unique_language(&mut fallbacks, "zh");
    }
    push_unique_language(&mut fallbacks, "en");
    fallbacks
}

fn push_unique_language(fallbacks: &mut Vec<String>, language: &str) {
    if language.is_empty() {
        return;
    }
    if !fallbacks.iter().any(|value| value == language) {
        fallbacks.push(language.to_string());
    }
}

fn should_report_table_progress(completed: usize, total: usize) -> bool {
    total > 0 && (completed == total || completed % TABLE_PROGRESS_REPORT_INTERVAL == 0)
}

fn market_groups_in_parent_order(rows: &[CatalogMarketGroup]) -> Vec<&CatalogMarketGroup> {
    let mut ordered = Vec::with_capacity(rows.len());
    let mut added = HashSet::with_capacity(rows.len());
    let all_ids: HashSet<i32> = rows.iter().map(|row| row.market_group_id).collect();

    while ordered.len() < rows.len() {
        let previous_len = ordered.len();
        for row in rows {
            if added.contains(&row.market_group_id) {
                continue;
            }
            if row.parent_group_id.map_or(true, |parent_id| {
                added.contains(&parent_id) || !all_ids.contains(&parent_id)
            }) {
                added.insert(row.market_group_id);
                ordered.push(row);
            }
        }

        if ordered.len() == previous_len {
            for row in rows {
                if added.insert(row.market_group_id) {
                    ordered.push(row);
                }
            }
        }
    }

    ordered
}

fn report_table_started<F>(progress: &mut F, table: CatalogImportTable, total: usize)
where
    F: FnMut(CatalogImportProgress),
{
    progress(CatalogImportProgress::TableStarted { table, total });
}

fn report_batch_started<F>(
    progress: &mut F,
    table: CatalogImportTable,
    completed: usize,
    total: usize,
    batch_size: usize,
    attempt: usize,
) where
    F: FnMut(CatalogImportProgress),
{
    progress(CatalogImportProgress::BatchStarted {
        table,
        completed,
        total,
        batch_size,
        attempt,
    });
}

fn report_batch_merging<F>(
    progress: &mut F,
    table: CatalogImportTable,
    completed: usize,
    total: usize,
    batch_size: usize,
    attempt: usize,
) where
    F: FnMut(CatalogImportProgress),
{
    progress(CatalogImportProgress::BatchMerging {
        table,
        completed,
        total,
        batch_size,
        attempt,
    });
}

fn report_batch_retrying<F>(
    progress: &mut F,
    table: CatalogImportTable,
    completed: usize,
    total: usize,
    batch_size: usize,
    next_attempt: usize,
    error: &sqlx::Error,
) where
    F: FnMut(CatalogImportProgress),
{
    progress(CatalogImportProgress::BatchRetrying {
        table,
        completed,
        total,
        batch_size,
        next_attempt,
        error_summary: error.to_string(),
    });
}

fn report_table_progress<F>(
    progress: &mut F,
    table: CatalogImportTable,
    completed: usize,
    total: usize,
) where
    F: FnMut(CatalogImportProgress),
{
    if should_report_table_progress(completed, total) {
        progress(CatalogImportProgress::TableAdvanced {
            table,
            completed,
            total,
        });
    }
}

fn import_staging_batches<T>(rows: &[T], chunk_size: usize) -> impl Iterator<Item = &[T]> {
    rows.chunks(chunk_size)
}

async fn import_categories(
    pool: &PgPool,
    import_id: i64,
    archive: &CatalogArchive,
    progress: &mut impl FnMut(CatalogImportProgress),
) -> Result<(), sqlx::Error> {
    let table = CatalogImportTable::Categories;
    let total = archive.categories.len();
    report_table_started(progress, table, total);
    copy_merge_rows_in_batches(
        pool,
        import_id,
        &archive.categories,
        StageSql {
            table_name: STAGE_CATEGORIES_TABLE,
            create_sql: CREATE_STAGE_CATEGORIES_SQL,
            copy_sql: COPY_STAGE_CATEGORIES_SQL,
            merge_sql: MERGE_STAGE_CATEGORIES_SQL,
        },
        COPY_CHUNK_SIZE,
        progress,
        table,
        total,
        write_category_copy_row,
    )
    .await
}

async fn import_category_localizations(
    pool: &PgPool,
    import_id: i64,
    archive: &CatalogArchive,
    progress: &mut impl FnMut(CatalogImportProgress),
) -> Result<(), sqlx::Error> {
    let table = CatalogImportTable::CategoryLocalizations;
    let total = archive
        .categories
        .iter()
        .map(|row| row.localizations.len())
        .sum();
    report_table_started(progress, table, total);
    let rows: Vec<_> = archive
        .categories
        .iter()
        .flat_map(|row| {
            row.localizations
                .iter()
                .map(move |localization| (row.category_id, localization))
        })
        .collect();
    copy_merge_rows_in_batches(
        pool,
        import_id,
        &rows,
        StageSql {
            table_name: STAGE_CATEGORY_LOCALIZATIONS_TABLE,
            create_sql: CREATE_STAGE_CATEGORY_LOCALIZATIONS_SQL,
            copy_sql: COPY_STAGE_CATEGORY_LOCALIZATIONS_SQL,
            merge_sql: MERGE_STAGE_CATEGORY_LOCALIZATIONS_SQL,
        },
        COPY_CHUNK_SIZE,
        progress,
        table,
        total,
        write_category_localization_copy_row,
    )
    .await
}

async fn import_groups(
    pool: &PgPool,
    import_id: i64,
    archive: &CatalogArchive,
    progress: &mut impl FnMut(CatalogImportProgress),
) -> Result<(), sqlx::Error> {
    let table = CatalogImportTable::Groups;
    let total = archive.groups.len();
    report_table_started(progress, table, total);
    copy_merge_rows_in_batches(
        pool,
        import_id,
        &archive.groups,
        StageSql {
            table_name: STAGE_GROUPS_TABLE,
            create_sql: CREATE_STAGE_GROUPS_SQL,
            copy_sql: COPY_STAGE_GROUPS_SQL,
            merge_sql: MERGE_STAGE_GROUPS_SQL,
        },
        COPY_CHUNK_SIZE,
        progress,
        table,
        total,
        write_group_copy_row,
    )
    .await
}

async fn import_group_localizations(
    pool: &PgPool,
    import_id: i64,
    archive: &CatalogArchive,
    progress: &mut impl FnMut(CatalogImportProgress),
) -> Result<(), sqlx::Error> {
    let table = CatalogImportTable::GroupLocalizations;
    let total = archive
        .groups
        .iter()
        .map(|row| row.localizations.len())
        .sum();
    report_table_started(progress, table, total);
    let rows: Vec<_> = archive
        .groups
        .iter()
        .flat_map(|row| {
            row.localizations
                .iter()
                .map(move |localization| (row.group_id, localization))
        })
        .collect();
    copy_merge_rows_in_batches(
        pool,
        import_id,
        &rows,
        StageSql {
            table_name: STAGE_GROUP_LOCALIZATIONS_TABLE,
            create_sql: CREATE_STAGE_GROUP_LOCALIZATIONS_SQL,
            copy_sql: COPY_STAGE_GROUP_LOCALIZATIONS_SQL,
            merge_sql: MERGE_STAGE_GROUP_LOCALIZATIONS_SQL,
        },
        COPY_CHUNK_SIZE,
        progress,
        table,
        total,
        write_group_localization_copy_row,
    )
    .await
}

async fn import_market_groups(
    pool: &PgPool,
    import_id: i64,
    archive: &CatalogArchive,
    progress: &mut impl FnMut(CatalogImportProgress),
) -> Result<(), sqlx::Error> {
    let table = CatalogImportTable::MarketGroups;
    let total = archive.market_groups.len();
    report_table_started(progress, table, total);
    let rows = market_groups_in_parent_order(&archive.market_groups);
    copy_merge_rows_in_batches(
        pool,
        import_id,
        &rows,
        StageSql {
            table_name: STAGE_MARKET_GROUPS_TABLE,
            create_sql: CREATE_STAGE_MARKET_GROUPS_SQL,
            copy_sql: COPY_STAGE_MARKET_GROUPS_SQL,
            merge_sql: MERGE_STAGE_MARKET_GROUPS_SQL,
        },
        COPY_CHUNK_SIZE,
        progress,
        table,
        total,
        write_market_group_copy_row,
    )
    .await
}

async fn import_market_group_localizations(
    pool: &PgPool,
    import_id: i64,
    archive: &CatalogArchive,
    progress: &mut impl FnMut(CatalogImportProgress),
) -> Result<(), sqlx::Error> {
    let table = CatalogImportTable::MarketGroupLocalizations;
    let total = archive
        .market_groups
        .iter()
        .map(|row| row.localizations.len())
        .sum();
    report_table_started(progress, table, total);
    let rows: Vec<_> = archive
        .market_groups
        .iter()
        .flat_map(|row| {
            row.localizations
                .iter()
                .map(move |localization| (row.market_group_id, localization))
        })
        .collect();
    copy_merge_rows_in_batches(
        pool,
        import_id,
        &rows,
        StageSql {
            table_name: STAGE_MARKET_GROUP_LOCALIZATIONS_TABLE,
            create_sql: CREATE_STAGE_MARKET_GROUP_LOCALIZATIONS_SQL,
            copy_sql: COPY_STAGE_MARKET_GROUP_LOCALIZATIONS_SQL,
            merge_sql: MERGE_STAGE_MARKET_GROUP_LOCALIZATIONS_SQL,
        },
        COPY_CHUNK_SIZE,
        progress,
        table,
        total,
        write_market_group_localization_copy_row,
    )
    .await
}

async fn import_types(
    pool: &PgPool,
    import_id: i64,
    archive: &CatalogArchive,
    progress: &mut impl FnMut(CatalogImportProgress),
) -> Result<(), sqlx::Error> {
    let table = CatalogImportTable::Types;
    let total = archive.types.len();
    report_table_started(progress, table, total);
    copy_merge_rows_in_batches(
        pool,
        import_id,
        &archive.types,
        StageSql {
            table_name: STAGE_TYPES_TABLE,
            create_sql: CREATE_STAGE_TYPES_SQL,
            copy_sql: COPY_STAGE_TYPES_SQL,
            merge_sql: MERGE_STAGE_TYPES_SQL,
        },
        TYPE_COPY_CHUNK_SIZE,
        progress,
        table,
        total,
        write_type_copy_row,
    )
    .await
}

async fn import_type_localizations(
    pool: &PgPool,
    import_id: i64,
    archive: &CatalogArchive,
    progress: &mut impl FnMut(CatalogImportProgress),
) -> Result<(), sqlx::Error> {
    let table = CatalogImportTable::TypeLocalizations;
    let total = archive
        .types
        .iter()
        .map(|row| row.localizations.len())
        .sum();
    report_table_started(progress, table, total);
    let rows: Vec<_> = archive
        .types
        .iter()
        .flat_map(|row| {
            row.localizations
                .iter()
                .map(move |localization| (row.type_id, localization))
        })
        .collect();
    copy_merge_rows_in_batches(
        pool,
        import_id,
        &rows,
        StageSql {
            table_name: STAGE_TYPE_LOCALIZATIONS_TABLE,
            create_sql: CREATE_STAGE_TYPE_LOCALIZATIONS_SQL,
            copy_sql: COPY_STAGE_TYPE_LOCALIZATIONS_SQL,
            merge_sql: MERGE_STAGE_TYPE_LOCALIZATIONS_SQL,
        },
        COPY_CHUNK_SIZE,
        progress,
        table,
        total,
        write_type_localization_copy_row,
    )
    .await
}

const COPY_CHUNK_SIZE: usize = 1_000;
const TYPE_COPY_CHUNK_SIZE: usize = 250;

const STAGE_CATEGORIES_TABLE: &str = "evetools_stage_inventory_categories";
const CREATE_STAGE_CATEGORIES_SQL: &str = r#"CREATE TEMP TABLE evetools_stage_inventory_categories (
    category_id INTEGER NOT NULL,
    published BOOLEAN NOT NULL,
    name_en TEXT,
    name_zh TEXT,
    raw_name_json JSONB NOT NULL,
    updated_import_id BIGINT NOT NULL
)"#;
const COPY_STAGE_CATEGORIES_SQL: &str = r#"COPY evetools_stage_inventory_categories
    (category_id, published, name_en, name_zh, raw_name_json, updated_import_id)
    FROM STDIN WITH (FORMAT CSV, NULL '\N')"#;
const MERGE_STAGE_CATEGORIES_SQL: &str = r#"INSERT INTO evetools_catalog.inventory_categories
    (category_id, published, name_en, name_zh, raw_name_json, updated_import_id)
SELECT category_id, published, name_en, name_zh, raw_name_json, updated_import_id
FROM evetools_stage_inventory_categories
ON CONFLICT (category_id) DO UPDATE SET
    published = EXCLUDED.published,
    name_en = EXCLUDED.name_en,
    name_zh = EXCLUDED.name_zh,
    raw_name_json = EXCLUDED.raw_name_json,
    updated_import_id = EXCLUDED.updated_import_id"#;

const STAGE_CATEGORY_LOCALIZATIONS_TABLE: &str = "evetools_stage_inventory_category_localizations";
const CREATE_STAGE_CATEGORY_LOCALIZATIONS_SQL: &str = r#"CREATE TEMP TABLE evetools_stage_inventory_category_localizations (
    category_id INTEGER NOT NULL,
    language TEXT NOT NULL,
    name TEXT,
    updated_import_id BIGINT NOT NULL
)"#;
const COPY_STAGE_CATEGORY_LOCALIZATIONS_SQL: &str = r#"COPY evetools_stage_inventory_category_localizations
    (category_id, language, name, updated_import_id)
    FROM STDIN WITH (FORMAT CSV, NULL '\N')"#;
const MERGE_STAGE_CATEGORY_LOCALIZATIONS_SQL: &str = r#"INSERT INTO evetools_catalog.inventory_category_localizations
    (category_id, language, name, updated_import_id)
SELECT category_id, language, name, updated_import_id
FROM evetools_stage_inventory_category_localizations
ON CONFLICT (category_id, language) DO UPDATE SET
    name = EXCLUDED.name,
    updated_import_id = EXCLUDED.updated_import_id"#;

const STAGE_GROUPS_TABLE: &str = "evetools_stage_inventory_groups";
const CREATE_STAGE_GROUPS_SQL: &str = r#"CREATE TEMP TABLE evetools_stage_inventory_groups (
    group_id INTEGER NOT NULL,
    category_id INTEGER NOT NULL,
    published BOOLEAN NOT NULL,
    name_en TEXT,
    name_zh TEXT,
    raw_name_json JSONB NOT NULL,
    updated_import_id BIGINT NOT NULL
)"#;
const COPY_STAGE_GROUPS_SQL: &str = r#"COPY evetools_stage_inventory_groups
    (group_id, category_id, published, name_en, name_zh, raw_name_json, updated_import_id)
    FROM STDIN WITH (FORMAT CSV, NULL '\N')"#;
const MERGE_STAGE_GROUPS_SQL: &str = r#"INSERT INTO evetools_catalog.inventory_groups
    (group_id, category_id, published, name_en, name_zh, raw_name_json, updated_import_id)
SELECT group_id, category_id, published, name_en, name_zh, raw_name_json, updated_import_id
FROM evetools_stage_inventory_groups
ON CONFLICT (group_id) DO UPDATE SET
    category_id = EXCLUDED.category_id,
    published = EXCLUDED.published,
    name_en = EXCLUDED.name_en,
    name_zh = EXCLUDED.name_zh,
    raw_name_json = EXCLUDED.raw_name_json,
    updated_import_id = EXCLUDED.updated_import_id"#;

const STAGE_GROUP_LOCALIZATIONS_TABLE: &str = "evetools_stage_inventory_group_localizations";
const CREATE_STAGE_GROUP_LOCALIZATIONS_SQL: &str = r#"CREATE TEMP TABLE evetools_stage_inventory_group_localizations (
    group_id INTEGER NOT NULL,
    language TEXT NOT NULL,
    name TEXT,
    updated_import_id BIGINT NOT NULL
)"#;
const COPY_STAGE_GROUP_LOCALIZATIONS_SQL: &str = r#"COPY evetools_stage_inventory_group_localizations
    (group_id, language, name, updated_import_id)
    FROM STDIN WITH (FORMAT CSV, NULL '\N')"#;
const MERGE_STAGE_GROUP_LOCALIZATIONS_SQL: &str = r#"INSERT INTO evetools_catalog.inventory_group_localizations
    (group_id, language, name, updated_import_id)
SELECT group_id, language, name, updated_import_id
FROM evetools_stage_inventory_group_localizations
ON CONFLICT (group_id, language) DO UPDATE SET
    name = EXCLUDED.name,
    updated_import_id = EXCLUDED.updated_import_id"#;

const STAGE_MARKET_GROUPS_TABLE: &str = "evetools_stage_market_groups";
const CREATE_STAGE_MARKET_GROUPS_SQL: &str = r#"CREATE TEMP TABLE evetools_stage_market_groups (
    market_group_id INTEGER NOT NULL,
    parent_group_id INTEGER,
    name_en TEXT,
    name_zh TEXT,
    description_en TEXT,
    description_zh TEXT,
    raw_name_json JSONB NOT NULL,
    raw_description_json JSONB,
    updated_import_id BIGINT NOT NULL
)"#;
const COPY_STAGE_MARKET_GROUPS_SQL: &str = r#"COPY evetools_stage_market_groups
    (market_group_id, parent_group_id, name_en, name_zh, description_en, description_zh,
     raw_name_json, raw_description_json, updated_import_id)
    FROM STDIN WITH (FORMAT CSV, NULL '\N')"#;
const MERGE_STAGE_MARKET_GROUPS_SQL: &str = r#"INSERT INTO evetools_catalog.market_groups
    (market_group_id, parent_group_id, name_en, name_zh, description_en, description_zh,
     raw_name_json, raw_description_json, updated_import_id)
SELECT market_group_id, parent_group_id, name_en, name_zh, description_en, description_zh,
       raw_name_json, raw_description_json, updated_import_id
FROM evetools_stage_market_groups
ON CONFLICT (market_group_id) DO UPDATE SET
    parent_group_id = EXCLUDED.parent_group_id,
    name_en = EXCLUDED.name_en,
    name_zh = EXCLUDED.name_zh,
    description_en = EXCLUDED.description_en,
    description_zh = EXCLUDED.description_zh,
    raw_name_json = EXCLUDED.raw_name_json,
    raw_description_json = EXCLUDED.raw_description_json,
    updated_import_id = EXCLUDED.updated_import_id"#;

const STAGE_MARKET_GROUP_LOCALIZATIONS_TABLE: &str = "evetools_stage_market_group_localizations";
const CREATE_STAGE_MARKET_GROUP_LOCALIZATIONS_SQL: &str = r#"CREATE TEMP TABLE evetools_stage_market_group_localizations (
    market_group_id INTEGER NOT NULL,
    language TEXT NOT NULL,
    name TEXT,
    description TEXT,
    updated_import_id BIGINT NOT NULL
)"#;
const COPY_STAGE_MARKET_GROUP_LOCALIZATIONS_SQL: &str = r#"COPY evetools_stage_market_group_localizations
    (market_group_id, language, name, description, updated_import_id)
    FROM STDIN WITH (FORMAT CSV, NULL '\N')"#;
const MERGE_STAGE_MARKET_GROUP_LOCALIZATIONS_SQL: &str = r#"INSERT INTO evetools_catalog.market_group_localizations
    (market_group_id, language, name, description, updated_import_id)
SELECT market_group_id, language, name, description, updated_import_id
FROM evetools_stage_market_group_localizations
ON CONFLICT (market_group_id, language) DO UPDATE SET
    name = EXCLUDED.name,
    description = EXCLUDED.description,
    updated_import_id = EXCLUDED.updated_import_id"#;

const STAGE_TYPES_TABLE: &str = "evetools_stage_inventory_types";
const CREATE_STAGE_TYPES_SQL: &str = r#"CREATE TEMP TABLE evetools_stage_inventory_types (
    type_id INTEGER NOT NULL,
    group_id INTEGER NOT NULL,
    market_group_id INTEGER,
    published BOOLEAN NOT NULL,
    volume DOUBLE PRECISION,
    packaged_volume DOUBLE PRECISION,
    capacity DOUBLE PRECISION,
    mass DOUBLE PRECISION,
    portion_size INTEGER,
    meta_level INTEGER,
    name_en TEXT,
    name_zh TEXT,
    description_en TEXT,
    description_zh TEXT,
    raw_name_json JSONB NOT NULL,
    raw_description_json JSONB,
    updated_import_id BIGINT NOT NULL
)"#;
const COPY_STAGE_TYPES_SQL: &str = r#"COPY evetools_stage_inventory_types
    (type_id, group_id, market_group_id, published, volume, packaged_volume, capacity,
     mass, portion_size, meta_level, name_en, name_zh, description_en, description_zh,
     raw_name_json, raw_description_json, updated_import_id)
    FROM STDIN WITH (FORMAT CSV, NULL '\N')"#;
const MERGE_STAGE_TYPES_SQL: &str = r#"INSERT INTO evetools_catalog.inventory_types
    (type_id, group_id, market_group_id, published, volume, packaged_volume, capacity,
     mass, portion_size, meta_level, name_en, name_zh, description_en, description_zh,
     raw_name_json, raw_description_json, updated_import_id)
SELECT type_id, group_id, market_group_id, published, volume, packaged_volume, capacity,
       mass, portion_size, meta_level, name_en, name_zh, description_en, description_zh,
       raw_name_json, raw_description_json, updated_import_id
FROM evetools_stage_inventory_types
ON CONFLICT (type_id) DO UPDATE SET
    group_id = EXCLUDED.group_id,
    market_group_id = EXCLUDED.market_group_id,
    published = EXCLUDED.published,
    volume = EXCLUDED.volume,
    packaged_volume = EXCLUDED.packaged_volume,
    capacity = EXCLUDED.capacity,
    mass = EXCLUDED.mass,
    portion_size = EXCLUDED.portion_size,
    meta_level = EXCLUDED.meta_level,
    name_en = EXCLUDED.name_en,
    name_zh = EXCLUDED.name_zh,
    description_en = EXCLUDED.description_en,
    description_zh = EXCLUDED.description_zh,
    raw_name_json = EXCLUDED.raw_name_json,
    raw_description_json = EXCLUDED.raw_description_json,
    updated_import_id = EXCLUDED.updated_import_id"#;

const STAGE_TYPE_LOCALIZATIONS_TABLE: &str = "evetools_stage_inventory_type_localizations";
const CREATE_STAGE_TYPE_LOCALIZATIONS_SQL: &str = r#"CREATE TEMP TABLE evetools_stage_inventory_type_localizations (
    type_id INTEGER NOT NULL,
    language TEXT NOT NULL,
    name TEXT,
    description TEXT,
    updated_import_id BIGINT NOT NULL
)"#;
const COPY_STAGE_TYPE_LOCALIZATIONS_SQL: &str = r#"COPY evetools_stage_inventory_type_localizations
    (type_id, language, name, description, updated_import_id)
    FROM STDIN WITH (FORMAT CSV, NULL '\N')"#;
const MERGE_STAGE_TYPE_LOCALIZATIONS_SQL: &str = r#"INSERT INTO evetools_catalog.inventory_type_localizations
    (type_id, language, name, description, updated_import_id)
SELECT type_id, language, name, description, updated_import_id
FROM evetools_stage_inventory_type_localizations
ON CONFLICT (type_id, language) DO UPDATE SET
    name = EXCLUDED.name,
    description = EXCLUDED.description,
    updated_import_id = EXCLUDED.updated_import_id"#;

#[derive(Clone, Copy)]
struct StageSql {
    table_name: &'static str,
    create_sql: &'static str,
    copy_sql: &'static str,
    merge_sql: &'static str,
}

async fn recreate_staging_table(
    connection: &mut PoolConnection<Postgres>,
    table_name: &str,
    create_sql: &str,
) -> Result<(), sqlx::Error> {
    let drop_sql = format!("DROP TABLE IF EXISTS pg_temp.{table_name}");
    sqlx::query(&drop_sql)
        .persistent(false)
        .execute(&mut **connection)
        .await?;
    sqlx::query(create_sql)
        .persistent(false)
        .execute(&mut **connection)
        .await?;
    Ok(())
}

async fn copy_merge_rows_in_batches<T>(
    pool: &PgPool,
    import_id: i64,
    rows: &[T],
    stage: StageSql,
    chunk_size: usize,
    progress: &mut impl FnMut(CatalogImportProgress),
    table: CatalogImportTable,
    total: usize,
    mut write_row: impl FnMut(&mut String, &T, i64),
) -> Result<(), sqlx::Error> {
    let mut completed = 0;
    for chunk in import_staging_batches(rows, chunk_size) {
        let mut attempt = 1;
        loop {
            match copy_merge_batch(
                pool,
                import_id,
                chunk,
                stage,
                progress,
                table,
                total,
                completed,
                attempt,
                &mut write_row,
            )
            .await
            {
                Ok(()) => break,
                Err(error) if attempt >= IMPORT_TABLE_MAX_ATTEMPTS => return Err(error),
                Err(error) => {
                    attempt += 1;
                    report_batch_retrying(
                        progress,
                        table,
                        completed,
                        total,
                        chunk.len(),
                        attempt,
                        &error,
                    );
                }
            }
        }

        completed += chunk.len();
        report_table_progress(progress, table, completed, total);
    }
    Ok(())
}

async fn copy_merge_batch<T>(
    pool: &PgPool,
    import_id: i64,
    rows: &[T],
    stage: StageSql,
    progress: &mut impl FnMut(CatalogImportProgress),
    table: CatalogImportTable,
    total: usize,
    completed: usize,
    attempt: usize,
    write_row: &mut impl FnMut(&mut String, &T, i64),
) -> Result<(), sqlx::Error> {
    let mut connection = pool.acquire().await?;
    recreate_staging_table(&mut connection, stage.table_name, stage.create_sql).await?;
    copy_rows(
        &mut connection,
        stage.copy_sql,
        rows,
        import_id,
        progress,
        table,
        total,
        completed,
        attempt,
        write_row,
    )
    .await?;
    merge_staged_rows(
        &mut connection,
        stage.merge_sql,
        progress,
        table,
        total,
        completed,
        rows.len(),
        attempt,
    )
    .await
}

async fn copy_rows<T>(
    connection: &mut PoolConnection<Postgres>,
    copy_sql: &str,
    rows: &[T],
    import_id: i64,
    progress: &mut impl FnMut(CatalogImportProgress),
    table: CatalogImportTable,
    total: usize,
    completed: usize,
    attempt: usize,
    write_row: &mut impl FnMut(&mut String, &T, i64),
) -> Result<(), sqlx::Error> {
    let mut copy = connection.copy_in_raw(copy_sql).await?;
    let mut buffer = String::with_capacity(1024 * 1024);
    report_batch_started(progress, table, completed, total, rows.len(), attempt);
    for row in rows {
        write_row(&mut buffer, row, import_id);
    }
    copy.send(buffer.as_bytes()).await?;
    copy.finish().await?;
    Ok(())
}

async fn merge_staged_rows(
    connection: &mut PoolConnection<Postgres>,
    merge_sql: &str,
    progress: &mut impl FnMut(CatalogImportProgress),
    table: CatalogImportTable,
    total: usize,
    completed: usize,
    batch_size: usize,
    attempt: usize,
) -> Result<(), sqlx::Error> {
    report_batch_merging(progress, table, completed, total, batch_size, attempt);
    sqlx::query(merge_sql)
        .persistent(false)
        .execute(&mut **connection)
        .await?;
    Ok(())
}

fn write_category_copy_row(buffer: &mut String, row: &CatalogCategory, import_id: i64) {
    push_copy_i32(buffer, row.category_id);
    push_copy_separator(buffer);
    push_copy_bool(buffer, row.published);
    push_copy_separator(buffer);
    push_copy_cell(buffer, row.name_en.as_deref());
    push_copy_separator(buffer);
    push_copy_cell(buffer, row.name_zh.as_deref());
    push_copy_separator(buffer);
    push_copy_json(buffer, &row.raw_name_json);
    push_copy_separator(buffer);
    push_copy_i64(buffer, import_id);
    finish_copy_row(buffer);
}

fn write_category_localization_copy_row(
    buffer: &mut String,
    (category_id, localization): &(i32, &CatalogLocalization),
    import_id: i64,
) {
    push_copy_i32(buffer, *category_id);
    push_copy_separator(buffer);
    write_localization_name_columns(buffer, localization, import_id);
}

fn write_group_copy_row(buffer: &mut String, row: &CatalogGroup, import_id: i64) {
    push_copy_i32(buffer, row.group_id);
    push_copy_separator(buffer);
    push_copy_i32(buffer, row.category_id);
    push_copy_separator(buffer);
    push_copy_bool(buffer, row.published);
    push_copy_separator(buffer);
    push_copy_cell(buffer, row.name_en.as_deref());
    push_copy_separator(buffer);
    push_copy_cell(buffer, row.name_zh.as_deref());
    push_copy_separator(buffer);
    push_copy_json(buffer, &row.raw_name_json);
    push_copy_separator(buffer);
    push_copy_i64(buffer, import_id);
    finish_copy_row(buffer);
}

fn write_group_localization_copy_row(
    buffer: &mut String,
    (group_id, localization): &(i32, &CatalogLocalization),
    import_id: i64,
) {
    push_copy_i32(buffer, *group_id);
    push_copy_separator(buffer);
    write_localization_name_columns(buffer, localization, import_id);
}

fn write_market_group_copy_row(buffer: &mut String, row: &&CatalogMarketGroup, import_id: i64) {
    let row = *row;
    push_copy_i32(buffer, row.market_group_id);
    push_copy_separator(buffer);
    push_copy_optional_i32(buffer, row.parent_group_id);
    push_copy_separator(buffer);
    push_copy_cell(buffer, row.name_en.as_deref());
    push_copy_separator(buffer);
    push_copy_cell(buffer, row.name_zh.as_deref());
    push_copy_separator(buffer);
    push_copy_cell(buffer, row.description_en.as_deref());
    push_copy_separator(buffer);
    push_copy_cell(buffer, row.description_zh.as_deref());
    push_copy_separator(buffer);
    push_copy_json(buffer, &row.raw_name_json);
    push_copy_separator(buffer);
    push_copy_optional_json(buffer, row.raw_description_json.as_ref());
    push_copy_separator(buffer);
    push_copy_i64(buffer, import_id);
    finish_copy_row(buffer);
}

fn write_market_group_localization_copy_row(
    buffer: &mut String,
    (market_group_id, localization): &(i32, &CatalogLocalization),
    import_id: i64,
) {
    push_copy_i32(buffer, *market_group_id);
    push_copy_separator(buffer);
    write_localization_name_description_columns(buffer, localization, import_id);
}

fn write_type_copy_row(buffer: &mut String, row: &CatalogType, import_id: i64) {
    push_copy_i32(buffer, row.type_id);
    push_copy_separator(buffer);
    push_copy_i32(buffer, row.group_id);
    push_copy_separator(buffer);
    push_copy_optional_i32(buffer, row.market_group_id);
    push_copy_separator(buffer);
    push_copy_bool(buffer, row.published);
    push_copy_separator(buffer);
    push_copy_optional_f64(buffer, row.volume);
    push_copy_separator(buffer);
    push_copy_optional_f64(buffer, row.packaged_volume);
    push_copy_separator(buffer);
    push_copy_optional_f64(buffer, row.capacity);
    push_copy_separator(buffer);
    push_copy_optional_f64(buffer, row.mass);
    push_copy_separator(buffer);
    push_copy_optional_i32(buffer, row.portion_size);
    push_copy_separator(buffer);
    push_copy_optional_i32(buffer, row.meta_level);
    push_copy_separator(buffer);
    push_copy_cell(buffer, row.name_en.as_deref());
    push_copy_separator(buffer);
    push_copy_cell(buffer, row.name_zh.as_deref());
    push_copy_separator(buffer);
    push_copy_cell(buffer, row.description_en.as_deref());
    push_copy_separator(buffer);
    push_copy_cell(buffer, row.description_zh.as_deref());
    push_copy_separator(buffer);
    push_copy_json(buffer, &row.raw_name_json);
    push_copy_separator(buffer);
    push_copy_optional_json(buffer, row.raw_description_json.as_ref());
    push_copy_separator(buffer);
    push_copy_i64(buffer, import_id);
    finish_copy_row(buffer);
}

fn write_type_localization_copy_row(
    buffer: &mut String,
    (type_id, localization): &(i32, &CatalogLocalization),
    import_id: i64,
) {
    push_copy_i32(buffer, *type_id);
    push_copy_separator(buffer);
    write_localization_name_description_columns(buffer, localization, import_id);
}

fn write_localization_name_columns(
    buffer: &mut String,
    localization: &CatalogLocalization,
    import_id: i64,
) {
    push_copy_cell(buffer, Some(localization.language.as_str()));
    push_copy_separator(buffer);
    push_copy_cell(buffer, localization.name.as_deref());
    push_copy_separator(buffer);
    push_copy_i64(buffer, import_id);
    finish_copy_row(buffer);
}

fn write_localization_name_description_columns(
    buffer: &mut String,
    localization: &CatalogLocalization,
    import_id: i64,
) {
    push_copy_cell(buffer, Some(localization.language.as_str()));
    push_copy_separator(buffer);
    push_copy_cell(buffer, localization.name.as_deref());
    push_copy_separator(buffer);
    push_copy_cell(buffer, localization.description.as_deref());
    push_copy_separator(buffer);
    push_copy_i64(buffer, import_id);
    finish_copy_row(buffer);
}

fn push_copy_separator(buffer: &mut String) {
    buffer.push(',');
}

fn finish_copy_row(buffer: &mut String) {
    buffer.push('\n');
}

fn push_copy_bool(buffer: &mut String, value: bool) {
    buffer.push_str(if value { "true" } else { "false" });
}

fn push_copy_i32(buffer: &mut String, value: i32) {
    buffer.push_str(&value.to_string());
}

fn push_copy_i64(buffer: &mut String, value: i64) {
    buffer.push_str(&value.to_string());
}

fn push_copy_optional_i32(buffer: &mut String, value: Option<i32>) {
    match value {
        Some(value) => push_copy_i32(buffer, value),
        None => push_copy_cell(buffer, None),
    }
}

fn push_copy_optional_f64(buffer: &mut String, value: Option<f64>) {
    match value {
        Some(value) => buffer.push_str(&value.to_string()),
        None => push_copy_cell(buffer, None),
    }
}

fn push_copy_json(buffer: &mut String, value: &serde_json::Value) {
    push_copy_cell(buffer, Some(&value.to_string()));
}

fn push_copy_optional_json(buffer: &mut String, value: Option<&serde_json::Value>) {
    match value {
        Some(value) => push_copy_json(buffer, value),
        None => push_copy_cell(buffer, None),
    }
}

fn push_copy_cell(buffer: &mut String, value: Option<&str>) {
    let Some(value) = value else {
        buffer.push_str(r"\N");
        return;
    };

    let needs_quotes = value == r"\N"
        || value.contains(',')
        || value.contains('"')
        || value.contains('\n')
        || value.contains('\r');
    if !needs_quotes {
        buffer.push_str(value);
        return;
    }

    buffer.push('"');
    for character in value.chars() {
        if character == '"' {
            buffer.push('"');
        }
        buffer.push(character);
    }
    buffer.push('"');
}

async fn delete_stale_catalog_rows(
    tx: &mut Transaction<'_, Postgres>,
    import_id: i64,
) -> Result<(), sqlx::Error> {
    for statement in DELETE_STALE_CATALOG_ROWS_STATEMENTS {
        sqlx::query(statement)
            .persistent(false)
            .bind(import_id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

async fn import_delete_stale_catalog_rows(
    pool: &PgPool,
    import_id: i64,
) -> Result<(), sqlx::Error> {
    let mut attempt = 1;
    loop {
        let mut tx = begin_import_transaction(pool).await?;
        match delete_stale_catalog_rows(&mut tx, import_id).await {
            Ok(()) => match tx.commit().await {
                Ok(()) => return Ok(()),
                Err(error) if attempt >= IMPORT_TABLE_MAX_ATTEMPTS => return Err(error),
                Err(_) => {
                    attempt += 1;
                    continue;
                }
            },
            Err(error) if attempt >= IMPORT_TABLE_MAX_ATTEMPTS => {
                let _ = tx.rollback().await;
                return Err(error);
            }
            Err(_) => {
                let _ = tx.rollback().await;
                attempt += 1;
                continue;
            }
        }
    }
}

async fn mark_import_failed(
    pool: &PgPool,
    import_id: i64,
    error_summary: &str,
) -> Result<(), sqlx::Error> {
    let error_summary: String = error_summary.chars().take(1_000).collect();
    sqlx::query(
        "UPDATE evetools_catalog.sde_imports
         SET completed_at = NOW(), status = 'failed', error_summary = $1
         WHERE import_id = $2",
    )
    .persistent(false)
    .bind(error_summary)
    .bind(import_id)
    .execute(pool)
    .await?;
    Ok(())
}

const DELETE_STALE_CATALOG_ROWS_STATEMENTS: &[&str] = &[
    "DELETE FROM evetools_catalog.inventory_type_localizations WHERE updated_import_id <> $1",
    "DELETE FROM evetools_catalog.market_group_localizations WHERE updated_import_id <> $1",
    "DELETE FROM evetools_catalog.inventory_group_localizations WHERE updated_import_id <> $1",
    "DELETE FROM evetools_catalog.inventory_category_localizations WHERE updated_import_id <> $1",
    "DELETE FROM evetools_catalog.inventory_types WHERE updated_import_id <> $1",
    "DELETE FROM evetools_catalog.market_groups WHERE updated_import_id <> $1",
    "DELETE FROM evetools_catalog.inventory_groups WHERE updated_import_id <> $1",
    "DELETE FROM evetools_catalog.inventory_categories WHERE updated_import_id <> $1",
];

#[cfg(test)]
mod tests {
    use super::*;
    use evetools_sde::SdeMetadata;

    fn empty_archive(build_number: i32) -> CatalogArchive {
        CatalogArchive {
            metadata: SdeMetadata {
                build_number: Some(build_number),
                release_date: Some("2026-05-19T12:12:31Z".to_string()),
            },
            types: Vec::new(),
            groups: Vec::new(),
            categories: Vec::new(),
            market_groups: Vec::new(),
        }
    }

    fn successful_status(build_number: i32, source_url: &str) -> CatalogStatus {
        CatalogStatus {
            status: "success".to_string(),
            build_number: Some(build_number),
            release_date: Some("2026-05-19T12:12:31Z".to_string()),
            source_url: Some(source_url.to_string()),
            completed_at: Some("2026-05-27T00:00:00Z".to_string()),
            error_summary: None,
            type_count: 0,
            group_count: 0,
            category_count: 0,
            market_group_count: 0,
        }
    }

    fn market_group(market_group_id: i32, parent_group_id: Option<i32>) -> CatalogMarketGroup {
        CatalogMarketGroup {
            market_group_id,
            parent_group_id,
            name_en: None,
            name_zh: None,
            description_en: None,
            description_zh: None,
            raw_name_json: serde_json::json!({}),
            raw_description_json: None,
            localizations: Vec::new(),
        }
    }

    #[test]
    fn search_limit_rejects_non_positive_values_and_clamps_large_values() {
        assert_eq!(search_limit(0), None);
        assert_eq!(search_limit(-10), None);
        assert_eq!(search_limit(1), Some(1));
        assert_eq!(search_limit(101), Some(100));
    }

    #[test]
    fn search_pattern_trims_and_escapes_ilike_wildcards() {
        assert_eq!(search_pattern("   "), None);
        assert_eq!(
            search_pattern(r"  %_\Mineral  "),
            Some(r"%\%\_\\Mineral%".to_string())
        );
    }

    #[test]
    fn import_reuse_rejects_same_build_when_source_differs() {
        let archive = empty_archive(3_351_823);
        let input = ImportCatalogInput {
            archive: &archive,
            source_url: "https://developers.eveonline.com/static-data/eve-online-static-data-latest-jsonl.zip",
        };
        let status = successful_status(3_351_823, "test://sample");

        assert!(!successful_import_matches_input(&status, &input));
    }

    #[test]
    fn import_reuse_rejects_same_build_when_counts_differ() {
        let archive = empty_archive(3_351_823);
        let input = ImportCatalogInput {
            archive: &archive,
            source_url: "test://same-build",
        };
        let mut status = successful_status(3_351_823, "test://same-build");
        status.type_count = 1;

        assert!(!successful_import_matches_input(&status, &input));
    }

    #[test]
    fn import_reuse_accepts_same_build_source_and_counts() {
        let archive = empty_archive(3_351_823);
        let input = ImportCatalogInput {
            archive: &archive,
            source_url: "test://same-build",
        };
        let status = successful_status(3_351_823, "test://same-build");

        assert!(successful_import_matches_input(&status, &input));
    }

    #[test]
    fn table_progress_reports_every_interval_and_final_row() {
        assert!(!should_report_table_progress(999, 2_500));
        assert!(should_report_table_progress(1_000, 2_500));
        assert!(!should_report_table_progress(1_001, 2_500));
        assert!(should_report_table_progress(2_000, 2_500));
        assert!(should_report_table_progress(2_500, 2_500));
    }

    #[test]
    fn market_groups_are_ordered_parent_first_for_batch_imports() {
        let rows = vec![
            market_group(30, Some(20)),
            market_group(20, Some(10)),
            market_group(10, None),
        ];
        let ordered_ids: Vec<i32> = market_groups_in_parent_order(&rows)
            .into_iter()
            .map(|row| row.market_group_id)
            .collect();

        assert_eq!(ordered_ids, vec![10, 20, 30]);
    }

    #[test]
    fn import_batches_use_implicit_statement_transactions_and_retries() {
        assert_eq!(IMPORT_BATCH_EXECUTION_SCOPE, "copy-staging");
        assert!(IMPORT_TABLE_MAX_ATTEMPTS > 1);
    }

    #[test]
    fn staging_batches_split_copy_and_merge_work() {
        let rows = vec![0; COPY_CHUNK_SIZE * 2 + 1];
        let batch_sizes: Vec<usize> = import_staging_batches(&rows, COPY_CHUNK_SIZE)
            .map(<[_]>::len)
            .collect();

        assert_eq!(batch_sizes, vec![COPY_CHUNK_SIZE, COPY_CHUNK_SIZE, 1]);
    }

    #[test]
    fn copy_csv_cells_escape_special_values_and_nulls() {
        let mut row = String::new();
        push_copy_cell(&mut row, Some("simple"));
        row.push(',');
        push_copy_cell(&mut row, None);
        row.push(',');
        push_copy_cell(&mut row, Some("a,b\"c\n"));

        assert_eq!(row, "simple,\\N,\"a,b\"\"c\n\"");
    }

    #[test]
    fn explicit_import_metadata_transactions_are_bounded() {
        assert!(IMPORT_TRANSACTION_SETUP_STATEMENTS
            .iter()
            .any(|statement| statement.contains("lock_timeout")));
        assert!(IMPORT_TRANSACTION_SETUP_STATEMENTS
            .iter()
            .any(|statement| statement.contains("idle_in_transaction_session_timeout")));
        assert!(IMPORT_TRANSACTION_SETUP_STATEMENTS
            .iter()
            .any(|statement| statement.contains("statement_timeout")));
    }

    #[test]
    fn expected_localization_counts_include_all_entity_tables() {
        let mut archive = empty_archive(3_351_823);
        archive.types.push(evetools_sde::CatalogType {
            type_id: 34,
            group_id: 18,
            market_group_id: None,
            published: true,
            volume: None,
            packaged_volume: None,
            capacity: None,
            mass: None,
            portion_size: None,
            meta_level: None,
            name_en: Some("Tritanium".to_string()),
            name_zh: None,
            description_en: None,
            description_zh: None,
            raw_name_json: serde_json::json!({"en":"Tritanium","ja":"トリタニウム"}),
            raw_description_json: None,
            localizations: vec![
                evetools_sde::CatalogLocalization {
                    language: "en".to_string(),
                    name: Some("Tritanium".to_string()),
                    description: None,
                },
                evetools_sde::CatalogLocalization {
                    language: "ja".to_string(),
                    name: Some("トリタニウム".to_string()),
                    description: None,
                },
            ],
        });

        assert_eq!(
            expected_localization_counts(&archive),
            CatalogLocalizationCounts {
                type_count: 2,
                group_count: 0,
                category_count: 0,
                market_group_count: 0,
            }
        );
    }

    #[test]
    fn language_fallbacks_prefer_exact_base_chinese_and_english() {
        assert_eq!(language_fallbacks("zh-Hans"), vec!["zh-Hans", "zh", "en"]);
        assert_eq!(language_fallbacks("en-US"), vec!["en-US", "en"]);
        assert_eq!(language_fallbacks(""), vec!["en"]);
        assert_eq!(language_fallbacks(" zh_CN "), vec!["zh-CN", "zh", "en"]);
    }
}
