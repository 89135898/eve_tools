use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, QueryBuilder};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthDbError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizedCharacter {
    pub character_id: i64,
    pub character_name: String,
    pub owner_hash: Option<String>,
    pub last_login_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterAuthToken {
    pub character_id: i64,
    pub refresh_token: String,
    pub access_token: Option<String>,
    pub access_token_expires_at: Option<String>,
    pub scopes: Vec<String>,
    pub token_type: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CharacterOrderSnapshotInput {
    pub sync_run_id: i64,
    pub character_id: i64,
    pub order_id: i64,
    pub type_id: i32,
    pub region_id: i32,
    pub location_id: i64,
    pub is_buy_order: bool,
    pub price: f64,
    pub volume_remain: i64,
    pub volume_total: i64,
    pub issued: String,
    pub duration: i32,
    pub min_volume: Option<i32>,
    pub order_range: String,
    pub is_corporation: bool,
    pub escrow: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CharacterOrderSnapshot {
    pub sync_run_id: i64,
    pub character_id: i64,
    pub order_id: i64,
    pub type_id: i32,
    pub region_id: i32,
    pub location_id: i64,
    pub is_buy_order: bool,
    pub price: f64,
    pub volume_remain: i64,
    pub volume_total: i64,
    pub issued: String,
    pub duration: i32,
    pub min_volume: Option<i32>,
    pub order_range: String,
    pub is_corporation: bool,
    pub escrow: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterOrderSyncSummary {
    pub sync_run_id: i64,
    pub character_id: i64,
    pub status: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub order_count: Option<i64>,
    pub error_summary: Option<String>,
}

#[derive(Clone)]
pub struct AuthRepository {
    pool: PgPool,
}

impl AuthRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn upsert_authorized_character(
        &self,
        character: &AuthorizedCharacter,
    ) -> Result<(), AuthDbError> {
        sqlx::query(
            "INSERT INTO evetools_catalog.characters
                (character_id, character_name, owner_hash, last_login_at, updated_at)
             VALUES ($1, $2, $3, $4::timestamptz, NOW())
             ON CONFLICT (character_id) DO UPDATE SET
                character_name = EXCLUDED.character_name,
                owner_hash = EXCLUDED.owner_hash,
                last_login_at = EXCLUDED.last_login_at,
                updated_at = NOW()",
        )
        .persistent(false)
        .bind(character.character_id)
        .bind(&character.character_name)
        .bind(character.owner_hash.as_deref())
        .bind(&character.last_login_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn upsert_auth_token(&self, token: &CharacterAuthToken) -> Result<(), AuthDbError> {
        sqlx::query(
            "INSERT INTO evetools_catalog.character_auth_tokens
                (character_id, refresh_token, access_token, access_token_expires_at, scopes, token_type, updated_at)
             VALUES ($1, $2, $3, $4::timestamptz, $5, $6, NOW())
             ON CONFLICT (character_id) DO UPDATE SET
                refresh_token = EXCLUDED.refresh_token,
                access_token = EXCLUDED.access_token,
                access_token_expires_at = EXCLUDED.access_token_expires_at,
                scopes = EXCLUDED.scopes,
                token_type = EXCLUDED.token_type,
                updated_at = NOW()",
        )
        .persistent(false)
        .bind(token.character_id)
        .bind(&token.refresh_token)
        .bind(token.access_token.as_deref())
        .bind(token.access_token_expires_at.as_deref())
        .bind(&token.scopes)
        .bind(&token.token_type)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn auth_token(
        &self,
        character_id: i64,
    ) -> Result<Option<CharacterAuthToken>, AuthDbError> {
        let row = sqlx::query_as::<_, CharacterAuthTokenRecord>(
            "SELECT character_id, refresh_token, access_token, access_token_expires_at, scopes, token_type
             FROM evetools_catalog.character_auth_tokens
             WHERE character_id = $1",
        )
        .persistent(false)
        .bind(character_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(character_auth_token_from_record))
    }

    pub async fn latest_authorized_character(
        &self,
    ) -> Result<Option<AuthorizedCharacter>, AuthDbError> {
        let row = sqlx::query_as::<_, AuthorizedCharacterRecord>(
            "SELECT character_id, character_name, owner_hash, last_login_at
             FROM evetools_catalog.characters
             ORDER BY last_login_at DESC, updated_at DESC, character_id
             LIMIT 1",
        )
        .persistent(false)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(authorized_character_from_record))
    }

    pub async fn start_character_order_sync(&self, character_id: i64) -> Result<i64, AuthDbError> {
        let sync_run_id = sqlx::query_scalar(
            "INSERT INTO evetools_catalog.character_order_sync_runs
                (character_id, started_at, status)
             VALUES ($1, NOW(), 'running')
             RETURNING sync_run_id",
        )
        .persistent(false)
        .bind(character_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(sync_run_id)
    }

    pub async fn replace_character_order_snapshots(
        &self,
        sync_run_id: i64,
        orders: &[CharacterOrderSnapshotInput],
    ) -> Result<(), AuthDbError> {
        sqlx::query(
            "DELETE FROM evetools_catalog.character_order_snapshots WHERE sync_run_id = $1",
        )
        .persistent(false)
        .bind(sync_run_id)
        .execute(&self.pool)
        .await?;

        if orders.is_empty() {
            return Ok(());
        }

        let mut query = QueryBuilder::<Postgres>::new(
            "INSERT INTO evetools_catalog.character_order_snapshots
                (sync_run_id, character_id, order_id, type_id, region_id, location_id,
                 is_buy_order, price, volume_remain, volume_total, issued, duration,
                 min_volume, order_range, is_corporation, escrow) ",
        );
        query.push_values(orders, |mut row_builder, order| {
            row_builder
                .push_bind(order.sync_run_id)
                .push_bind(order.character_id)
                .push_bind(order.order_id)
                .push_bind(order.type_id)
                .push_bind(order.region_id)
                .push_bind(order.location_id)
                .push_bind(order.is_buy_order)
                .push_bind(order.price)
                .push_bind(order.volume_remain)
                .push_bind(order.volume_total)
                .push_bind(&order.issued)
                .push_bind(order.duration)
                .push_bind(order.min_volume)
                .push_bind(&order.order_range)
                .push_bind(order.is_corporation)
                .push_bind(order.escrow);
        });
        query.build().persistent(false).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn complete_character_order_sync(
        &self,
        sync_run_id: i64,
        order_count: i64,
    ) -> Result<(), AuthDbError> {
        sqlx::query(
            "UPDATE evetools_catalog.character_order_sync_runs
             SET completed_at = NOW(), status = 'success', order_count = $1, error_summary = NULL
             WHERE sync_run_id = $2",
        )
        .persistent(false)
        .bind(order_count)
        .bind(sync_run_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn fail_character_order_sync(
        &self,
        sync_run_id: i64,
        error_summary: &str,
    ) -> Result<(), AuthDbError> {
        let redacted = redact_secret_text(error_summary);
        sqlx::query(
            "UPDATE evetools_catalog.character_order_sync_runs
             SET completed_at = NOW(), status = 'failed', error_summary = $1
             WHERE sync_run_id = $2",
        )
        .persistent(false)
        .bind(redacted)
        .bind(sync_run_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn latest_character_orders(
        &self,
        character_id: i64,
        limit: i64,
    ) -> Result<Vec<CharacterOrderSnapshot>, AuthDbError> {
        if limit <= 0 {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, CharacterOrderSnapshotRecord>(
            "WITH latest_run AS (
                SELECT sync_run_id
                FROM evetools_catalog.character_order_sync_runs
                WHERE character_id = $1 AND status = 'success'
                ORDER BY completed_at DESC NULLS LAST, sync_run_id DESC
                LIMIT 1
             )
             SELECT o.sync_run_id, o.character_id, o.order_id, o.type_id, o.region_id,
                    o.location_id, o.is_buy_order, o.price, o.volume_remain, o.volume_total,
                    o.issued, o.duration, o.min_volume, o.order_range, o.is_corporation, o.escrow
             FROM evetools_catalog.character_order_snapshots o
             JOIN latest_run lr ON lr.sync_run_id = o.sync_run_id
             WHERE o.character_id = $1
             ORDER BY o.order_id
             LIMIT $2",
        )
        .persistent(false)
        .bind(character_id)
        .bind(limit.min(10_000))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(character_order_snapshot_from_record)
            .collect())
    }

    pub async fn latest_character_order_sync(
        &self,
        character_id: i64,
    ) -> Result<Option<CharacterOrderSyncSummary>, AuthDbError> {
        let row = sqlx::query_as::<_, CharacterOrderSyncSummaryRecord>(
            "SELECT sync_run_id, character_id, status, started_at, completed_at, order_count, error_summary
             FROM evetools_catalog.character_order_sync_runs
             WHERE character_id = $1
             ORDER BY started_at DESC, sync_run_id DESC
             LIMIT 1",
        )
        .persistent(false)
        .bind(character_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(character_order_sync_summary_from_record))
    }
}

type CharacterAuthTokenRecord = (
    i64,
    String,
    Option<String>,
    Option<DateTime<Utc>>,
    Vec<String>,
    String,
);

type AuthorizedCharacterRecord = (i64, String, Option<String>, DateTime<Utc>);

type CharacterOrderSnapshotRecord = (
    i64,
    i64,
    i64,
    i32,
    i32,
    i64,
    bool,
    f64,
    i64,
    i64,
    String,
    i32,
    Option<i32>,
    String,
    bool,
    Option<f64>,
);

type CharacterOrderSyncSummaryRecord = (
    i64,
    i64,
    String,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
    Option<i64>,
    Option<String>,
);

fn character_auth_token_from_record(row: CharacterAuthTokenRecord) -> CharacterAuthToken {
    CharacterAuthToken {
        character_id: row.0,
        refresh_token: row.1,
        access_token: row.2,
        access_token_expires_at: row.3.map(|value| value.to_rfc3339()),
        scopes: row.4,
        token_type: row.5,
    }
}

fn authorized_character_from_record(row: AuthorizedCharacterRecord) -> AuthorizedCharacter {
    AuthorizedCharacter {
        character_id: row.0,
        character_name: row.1,
        owner_hash: row.2,
        last_login_at: row.3.to_rfc3339(),
    }
}

fn character_order_snapshot_from_record(
    row: CharacterOrderSnapshotRecord,
) -> CharacterOrderSnapshot {
    CharacterOrderSnapshot {
        sync_run_id: row.0,
        character_id: row.1,
        order_id: row.2,
        type_id: row.3,
        region_id: row.4,
        location_id: row.5,
        is_buy_order: row.6,
        price: row.7,
        volume_remain: row.8,
        volume_total: row.9,
        issued: row.10,
        duration: row.11,
        min_volume: row.12,
        order_range: row.13,
        is_corporation: row.14,
        escrow: row.15,
    }
}

fn character_order_sync_summary_from_record(
    row: CharacterOrderSyncSummaryRecord,
) -> CharacterOrderSyncSummary {
    CharacterOrderSyncSummary {
        sync_run_id: row.0,
        character_id: row.1,
        status: row.2,
        started_at: row.3.to_rfc3339(),
        completed_at: row.4.map(|value| value.to_rfc3339()),
        order_count: row.5,
        error_summary: row.6,
    }
}

fn redact_secret_text(value: &str) -> String {
    value
        .replace("access-token", "[redacted]")
        .replace("refresh-token", "[redacted]")
        .chars()
        .take(1_000)
        .collect()
}
