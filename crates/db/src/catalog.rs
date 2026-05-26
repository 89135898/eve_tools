use chrono::Utc;
use evetools_sde::CatalogArchive;
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgRow, PgPool, Postgres, Row, Transaction};
use thiserror::Error;

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

#[derive(Clone)]
pub struct CatalogRepository {
    pool: PgPool,
}

impl CatalogRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn latest_status(&self) -> Result<CatalogStatus, CatalogDbError> {
        let row = sqlx::query_as::<
            _,
            (
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
            ),
        >(
            "SELECT status, build_number, release_date, source_url, completed_at, error_summary,
                    type_count, group_count, category_count, market_group_count
             FROM evetools_catalog.sde_imports
             ORDER BY import_id DESC
             LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row
            .map(|row| CatalogStatus {
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
            })
            .unwrap_or(CatalogStatus {
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
            }))
    }

    pub async fn import_archive(
        &self,
        input: ImportCatalogInput<'_>,
    ) -> Result<CatalogStatus, CatalogDbError> {
        let mut tx = self.pool.begin().await?;
        let import_id: i64 = sqlx::query_scalar(
            "INSERT INTO evetools_catalog.sde_imports
                (build_number, release_date, source_url, started_at, status)
             VALUES ($1, $2, $3, NOW(), 'running')
             RETURNING import_id",
        )
        .bind(input.archive.metadata.build_number)
        .bind(input.archive.metadata.release_date.as_deref())
        .bind(input.source_url)
        .fetch_one(&mut *tx)
        .await?;

        insert_categories(&mut tx, import_id, input.archive).await?;
        insert_groups(&mut tx, import_id, input.archive).await?;
        insert_market_groups(&mut tx, import_id, input.archive).await?;
        insert_types(&mut tx, import_id, input.archive).await?;

        sqlx::query(
            "UPDATE evetools_catalog.sde_imports
             SET completed_at = NOW(), status = 'success',
                 type_count = $1, group_count = $2,
                 category_count = $3, market_group_count = $4
             WHERE import_id = $5",
        )
        .bind(input.archive.types.len() as i64)
        .bind(input.archive.groups.len() as i64)
        .bind(input.archive.categories.len() as i64)
        .bind(input.archive.market_groups.len() as i64)
        .bind(import_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        self.latest_status().await
    }

    pub async fn get_inventory_type(
        &self,
        type_id: i32,
        language: &str,
    ) -> Result<Option<InventoryTypeView>, CatalogDbError> {
        let row = sqlx::query_as::<_, InventoryTypeRow>(TYPE_SELECT_SQL)
            .bind(type_id)
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
        let pattern = format!("%{}%", query.trim());
        let rows = sqlx::query_as::<_, InventoryTypeRow>(
            "SELECT t.type_id, t.group_id, g.category_id, t.market_group_id,
                    t.name_en, t.name_zh, g.name_en AS group_name_en, g.name_zh AS group_name_zh,
                    c.name_en AS category_name_en, c.name_zh AS category_name_zh,
                    mg.name_en AS market_group_name_en, mg.name_zh AS market_group_name_zh,
                    t.published,
                    (t.published AND t.market_group_id IS NOT NULL AND (t.name_en IS NOT NULL OR t.name_zh IS NOT NULL)) AS market_eligible
             FROM evetools_catalog.inventory_types t
             LEFT JOIN evetools_catalog.inventory_groups g ON g.group_id = t.group_id
             LEFT JOIN evetools_catalog.inventory_categories c ON c.category_id = g.category_id
             LEFT JOIN evetools_catalog.market_groups mg ON mg.market_group_id = t.market_group_id
             WHERE t.name_en ILIKE $1 OR t.name_zh ILIKE $1
             ORDER BY t.name_en NULLS LAST
             LIMIT $2",
        )
        .bind(pattern)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| row.into_view(language))
            .collect())
    }
}

const TYPE_SELECT_SQL: &str = "SELECT t.type_id, t.group_id, g.category_id, t.market_group_id,
        t.name_en, t.name_zh, g.name_en AS group_name_en, g.name_zh AS group_name_zh,
        c.name_en AS category_name_en, c.name_zh AS category_name_zh,
        mg.name_en AS market_group_name_en, mg.name_zh AS market_group_name_zh,
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
    name_en: Option<String>,
    name_zh: Option<String>,
    group_name_en: Option<String>,
    group_name_zh: Option<String>,
    category_name_en: Option<String>,
    category_name_zh: Option<String>,
    market_group_name_en: Option<String>,
    market_group_name_zh: Option<String>,
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
            name_en: row.try_get("name_en")?,
            name_zh: row.try_get("name_zh")?,
            group_name_en: row.try_get("group_name_en")?,
            group_name_zh: row.try_get("group_name_zh")?,
            category_name_en: row.try_get("category_name_en")?,
            category_name_zh: row.try_get("category_name_zh")?,
            market_group_name_en: row.try_get("market_group_name_en")?,
            market_group_name_zh: row.try_get("market_group_name_zh")?,
            published: row.try_get("published")?,
            market_eligible: row.try_get("market_eligible")?,
        })
    }
}

impl InventoryTypeRow {
    fn into_view(self, language: &str) -> InventoryTypeView {
        let prefer_zh = language.starts_with("zh");
        let display_name = choose_name(prefer_zh, self.name_zh.as_ref(), self.name_en.as_ref())
            .unwrap_or_else(|| format!("Type {}", self.type_id));
        InventoryTypeView {
            type_id: self.type_id,
            group_id: self.group_id,
            category_id: self.category_id,
            market_group_id: self.market_group_id,
            display_name,
            name_en: self.name_en,
            name_zh: self.name_zh,
            group_name: choose_name(
                prefer_zh,
                self.group_name_zh.as_ref(),
                self.group_name_en.as_ref(),
            ),
            category_name: choose_name(
                prefer_zh,
                self.category_name_zh.as_ref(),
                self.category_name_en.as_ref(),
            ),
            market_group_name: choose_name(
                prefer_zh,
                self.market_group_name_zh.as_ref(),
                self.market_group_name_en.as_ref(),
            ),
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

async fn insert_categories(
    tx: &mut Transaction<'_, Postgres>,
    import_id: i64,
    archive: &CatalogArchive,
) -> Result<(), sqlx::Error> {
    for row in &archive.categories {
        sqlx::query(
            "INSERT INTO evetools_catalog.inventory_categories
                (category_id, published, name_en, name_zh, raw_name_json, updated_import_id)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (category_id) DO UPDATE SET
                published = EXCLUDED.published,
                name_en = EXCLUDED.name_en,
                name_zh = EXCLUDED.name_zh,
                raw_name_json = EXCLUDED.raw_name_json,
                updated_import_id = EXCLUDED.updated_import_id",
        )
        .bind(row.category_id)
        .bind(row.published)
        .bind(row.name_en.as_deref())
        .bind(row.name_zh.as_deref())
        .bind(&row.raw_name_json)
        .bind(import_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn insert_groups(
    tx: &mut Transaction<'_, Postgres>,
    import_id: i64,
    archive: &CatalogArchive,
) -> Result<(), sqlx::Error> {
    for row in &archive.groups {
        sqlx::query(
            "INSERT INTO evetools_catalog.inventory_groups
                (group_id, category_id, published, name_en, name_zh, raw_name_json, updated_import_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (group_id) DO UPDATE SET
                category_id = EXCLUDED.category_id,
                published = EXCLUDED.published,
                name_en = EXCLUDED.name_en,
                name_zh = EXCLUDED.name_zh,
                raw_name_json = EXCLUDED.raw_name_json,
                updated_import_id = EXCLUDED.updated_import_id",
        )
        .bind(row.group_id)
        .bind(row.category_id)
        .bind(row.published)
        .bind(row.name_en.as_deref())
        .bind(row.name_zh.as_deref())
        .bind(&row.raw_name_json)
        .bind(import_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn insert_market_groups(
    tx: &mut Transaction<'_, Postgres>,
    import_id: i64,
    archive: &CatalogArchive,
) -> Result<(), sqlx::Error> {
    for row in &archive.market_groups {
        sqlx::query(
            "INSERT INTO evetools_catalog.market_groups
                (market_group_id, parent_group_id, name_en, name_zh, description_en, description_zh,
                 raw_name_json, raw_description_json, updated_import_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (market_group_id) DO UPDATE SET
                parent_group_id = EXCLUDED.parent_group_id,
                name_en = EXCLUDED.name_en,
                name_zh = EXCLUDED.name_zh,
                description_en = EXCLUDED.description_en,
                description_zh = EXCLUDED.description_zh,
                raw_name_json = EXCLUDED.raw_name_json,
                raw_description_json = EXCLUDED.raw_description_json,
                updated_import_id = EXCLUDED.updated_import_id",
        )
        .bind(row.market_group_id)
        .bind(row.parent_group_id)
        .bind(row.name_en.as_deref())
        .bind(row.name_zh.as_deref())
        .bind(row.description_en.as_deref())
        .bind(row.description_zh.as_deref())
        .bind(&row.raw_name_json)
        .bind(row.raw_description_json.as_ref())
        .bind(import_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn insert_types(
    tx: &mut Transaction<'_, Postgres>,
    import_id: i64,
    archive: &CatalogArchive,
) -> Result<(), sqlx::Error> {
    for row in &archive.types {
        sqlx::query(
            "INSERT INTO evetools_catalog.inventory_types
                (type_id, group_id, market_group_id, published, volume, packaged_volume, capacity,
                 mass, portion_size, meta_level, name_en, name_zh, description_en, description_zh,
                 raw_name_json, raw_description_json, updated_import_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
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
                updated_import_id = EXCLUDED.updated_import_id",
        )
        .bind(row.type_id)
        .bind(row.group_id)
        .bind(row.market_group_id)
        .bind(row.published)
        .bind(row.volume)
        .bind(row.packaged_volume)
        .bind(row.capacity)
        .bind(row.mass)
        .bind(row.portion_size)
        .bind(row.meta_level)
        .bind(row.name_en.as_deref())
        .bind(row.name_zh.as_deref())
        .bind(row.description_en.as_deref())
        .bind(row.description_zh.as_deref())
        .bind(&row.raw_name_json)
        .bind(row.raw_description_json.as_ref())
        .bind(import_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}
