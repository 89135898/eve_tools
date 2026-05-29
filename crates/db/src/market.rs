use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, QueryBuilder};
use thiserror::Error;

const MAX_STATION_ORDER_BOOK_LIMIT: i64 = 500;

#[derive(Debug, Error)]
pub enum MarketDbError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("market sync run {sync_run_id} is not active or lease has expired")]
    InactiveSyncRun { sync_run_id: i64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradeHub {
    pub hub_id: String,
    pub display_name: String,
    pub region_id: i32,
    pub system_id: i32,
    pub station_id: i64,
    pub enabled: bool,
    pub sort_order: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MarketOrderSnapshotInput {
    pub sync_run_id: i64,
    pub region_id: i32,
    pub station_id: i64,
    pub type_id: i32,
    pub order_id: i64,
    pub is_buy_order: bool,
    pub price: f64,
    pub volume_remain: i64,
    pub volume_total: i64,
    pub issued: String,
    pub duration: i32,
    pub min_volume: i32,
    pub order_range: String,
    pub system_id: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MarketOrderSnapshot {
    pub sync_run_id: i64,
    pub region_id: i32,
    pub station_id: i64,
    pub type_id: i32,
    pub order_id: i64,
    pub is_buy_order: bool,
    pub price: f64,
    pub volume_remain: i64,
    pub volume_total: i64,
    pub issued: String,
    pub duration: i32,
    pub min_volume: i32,
    pub order_range: String,
    pub system_id: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StationOrderBook {
    pub sync_run_id: i64,
    pub region_id: i32,
    pub station_id: i64,
    pub type_id: i32,
    pub display_name: String,
    pub best_bid: f64,
    pub best_ask: f64,
    pub top_buy_depth: i64,
    pub top_sell_depth: i64,
    pub visible_volume: i64,
    pub last_synced_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarketSyncStartStatus {
    Started,
    AlreadyRunning,
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarketSyncHealthStatus {
    Fresh,
    Stale,
    Expired,
    Missing,
    Syncing,
    Degraded,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketSyncStartResult {
    pub status: MarketSyncStartStatus,
    pub sync_run_id: Option<i64>,
    pub region_id: i32,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketSyncHealthReport {
    pub generated_at: String,
    pub hubs: Vec<TradeHubSyncHealth>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradeHubSyncHealth {
    pub hub_id: String,
    pub display_name: String,
    pub region_id: i32,
    pub station_id: i64,
    pub status: MarketSyncHealthStatus,
    pub latest_success_sync_run_id: Option<i64>,
    pub latest_success_completed_at: Option<String>,
    pub latest_attempt_sync_run_id: Option<i64>,
    pub latest_attempt_status: Option<String>,
    pub latest_attempt_error: Option<String>,
    pub age_seconds: Option<i64>,
    pub order_count: Option<i64>,
    pub consecutive_failures: i64,
}

#[derive(Clone)]
pub struct MarketRepository {
    pool: PgPool,
}

impl MarketRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn upsert_trade_hubs(&self, hubs: &[TradeHub]) -> Result<(), MarketDbError> {
        if hubs.is_empty() {
            return Ok(());
        }

        let mut query = QueryBuilder::new(
            "INSERT INTO evetools_catalog.trade_hubs
                (hub_id, display_name, region_id, system_id, station_id, enabled, sort_order) ",
        );
        query.push_values(hubs, |mut row_builder, hub| {
            row_builder
                .push_bind(&hub.hub_id)
                .push_bind(&hub.display_name)
                .push_bind(hub.region_id)
                .push_bind(hub.system_id)
                .push_bind(hub.station_id)
                .push_bind(hub.enabled)
                .push_bind(hub.sort_order);
        });
        query.push(
            " ON CONFLICT (hub_id) DO UPDATE SET
                display_name = EXCLUDED.display_name,
                region_id = EXCLUDED.region_id,
                system_id = EXCLUDED.system_id,
                station_id = EXCLUDED.station_id,
                enabled = EXCLUDED.enabled,
                sort_order = EXCLUDED.sort_order",
        );
        query.build().persistent(false).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn list_enabled_trade_hubs(&self) -> Result<Vec<TradeHub>, MarketDbError> {
        let rows = sqlx::query_as::<_, TradeHubRecord>(
            "SELECT hub_id, display_name, region_id, system_id, station_id, enabled, sort_order
             FROM evetools_catalog.trade_hubs
             WHERE enabled = TRUE
             ORDER BY sort_order, hub_id",
        )
        .persistent(false)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(trade_hub_from_record).collect())
    }

    pub async fn start_sync_run(&self, region_id: i32, source: &str) -> Result<i64, MarketDbError> {
        let sync_run_id = sqlx::query_scalar(
            "INSERT INTO evetools_catalog.market_sync_runs
                (region_id, started_at, status, source)
             VALUES ($1, NOW(), 'running', $2)
             RETURNING sync_run_id",
        )
        .persistent(false)
        .bind(region_id)
        .bind(source)
        .fetch_one(&self.pool)
        .await?;
        Ok(sync_run_id)
    }

    pub async fn try_start_sync_run(
        &self,
        region_id: i32,
        source: &str,
        started_by: &str,
        lease_owner: &str,
        lease_ttl: Duration,
    ) -> Result<MarketSyncStartResult, MarketDbError> {
        self.expire_region_leases(region_id).await?;
        let lease_seconds = lease_ttl.num_seconds().max(1);
        let result = sqlx::query_scalar::<_, i64>(
            "INSERT INTO evetools_catalog.market_sync_runs
                (region_id, started_at, status, source, started_by, lease_owner, lease_expires_at)
             VALUES ($1, NOW(), 'leased', $2, $3, $4, NOW() + ($5::text || ' seconds')::interval)
             RETURNING sync_run_id",
        )
        .persistent(false)
        .bind(region_id)
        .bind(source)
        .bind(started_by)
        .bind(lease_owner)
        .bind(lease_seconds.to_string())
        .fetch_one(&self.pool)
        .await;

        match result {
            Ok(sync_run_id) => Ok(MarketSyncStartResult {
                status: MarketSyncStartStatus::Started,
                sync_run_id: Some(sync_run_id),
                region_id,
                message: "started".to_string(),
            }),
            Err(sqlx::Error::Database(error)) if error.code().as_deref() == Some("23505") => {
                let active_id = self.active_sync_run(region_id).await?;
                Ok(MarketSyncStartResult {
                    status: MarketSyncStartStatus::AlreadyRunning,
                    sync_run_id: active_id,
                    region_id,
                    message: "another sync is already running".to_string(),
                })
            }
            Err(error) => Err(error.into()),
        }
    }

    pub async fn mark_sync_run_running(&self, sync_run_id: i64) -> Result<(), MarketDbError> {
        let result = sqlx::query(
            "UPDATE evetools_catalog.market_sync_runs
             SET status = 'running'
             WHERE sync_run_id = $1
               AND status = 'leased'
               AND (lease_expires_at IS NULL OR lease_expires_at >= NOW())",
        )
        .persistent(false)
        .bind(sync_run_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(MarketDbError::InactiveSyncRun { sync_run_id });
        }
        Ok(())
    }

    pub async fn complete_sync_run(
        &self,
        sync_run_id: i64,
        page_count: i32,
        order_count: i64,
    ) -> Result<(), MarketDbError> {
        let result = sqlx::query(
            "UPDATE evetools_catalog.market_sync_runs
             SET completed_at = NOW(), status = 'success',
                 page_count = $1, order_count = $2, error_summary = NULL,
                 duration_ms = (EXTRACT(EPOCH FROM (NOW() - started_at)) * 1000)::BIGINT,
                 completed_reason = 'completed',
                 lease_owner = NULL,
                 lease_expires_at = NULL
             WHERE sync_run_id = $3
               AND status = 'running'
               AND (lease_expires_at IS NULL OR lease_expires_at >= NOW())",
        )
        .persistent(false)
        .bind(page_count)
        .bind(order_count)
        .bind(sync_run_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(MarketDbError::InactiveSyncRun { sync_run_id });
        }
        Ok(())
    }

    pub async fn fail_sync_run(
        &self,
        sync_run_id: i64,
        error_summary: &str,
    ) -> Result<(), MarketDbError> {
        let error_summary: String = error_summary.chars().take(1_000).collect();
        let result = sqlx::query(
            "UPDATE evetools_catalog.market_sync_runs
             SET completed_at = NOW(), status = 'failed', error_summary = $1,
                 duration_ms = (EXTRACT(EPOCH FROM (NOW() - started_at)) * 1000)::BIGINT,
                 completed_reason = 'failed',
                 lease_owner = NULL,
                 lease_expires_at = NULL
             WHERE sync_run_id = $2
               AND status IN ('leased', 'running')
               AND (lease_expires_at IS NULL OR lease_expires_at >= NOW())",
        )
        .persistent(false)
        .bind(error_summary)
        .bind(sync_run_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(MarketDbError::InactiveSyncRun { sync_run_id });
        }
        Ok(())
    }

    pub async fn replace_order_snapshots(
        &self,
        sync_run_id: i64,
        orders: &[MarketOrderSnapshotInput],
    ) -> Result<(), MarketDbError> {
        self.ensure_sync_run_can_write_snapshots(sync_run_id)
            .await?;

        sqlx::query("DELETE FROM evetools_catalog.market_order_snapshots WHERE sync_run_id = $1")
            .persistent(false)
            .bind(sync_run_id)
            .execute(&self.pool)
            .await?;

        if orders.is_empty() {
            return Ok(());
        }

        let mut query = QueryBuilder::<Postgres>::new(
            "INSERT INTO evetools_catalog.market_order_snapshots
                (sync_run_id, region_id, station_id, type_id, order_id, is_buy_order, price,
                 volume_remain, volume_total, issued, duration, min_volume, order_range, system_id) ",
        );
        query.push_values(orders, |mut row_builder, order| {
            row_builder
                .push_bind(order.sync_run_id)
                .push_bind(order.region_id)
                .push_bind(order.station_id)
                .push_bind(order.type_id)
                .push_bind(order.order_id)
                .push_bind(order.is_buy_order)
                .push_bind(order.price)
                .push_bind(order.volume_remain)
                .push_bind(order.volume_total)
                .push_bind(&order.issued)
                .push_bind(order.duration)
                .push_bind(order.min_volume)
                .push_bind(&order.order_range)
                .push_bind(order.system_id);
        });
        query.build().persistent(false).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn latest_station_orders(
        &self,
        region_id: i32,
        station_id: i64,
        limit: i64,
    ) -> Result<Vec<MarketOrderSnapshot>, MarketDbError> {
        if limit <= 0 {
            return Ok(Vec::new());
        }

        let Some(sync_run_id) = self.latest_successful_sync_run(region_id).await? else {
            return Ok(Vec::new());
        };
        let rows = sqlx::query_as::<_, MarketOrderSnapshotRecord>(
            "SELECT sync_run_id, region_id, station_id, type_id, order_id, is_buy_order, price,
                    volume_remain, volume_total, issued, duration, min_volume, order_range, system_id
             FROM evetools_catalog.market_order_snapshots
             WHERE sync_run_id = $1 AND station_id = $2
             ORDER BY type_id, is_buy_order DESC, price
             LIMIT $3",
        )
        .persistent(false)
        .bind(sync_run_id)
        .bind(station_id)
        .bind(limit.min(10_000))
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(market_order_from_record).collect())
    }

    pub async fn latest_station_order_books(
        &self,
        region_id: i32,
        station_id: i64,
        language: &str,
        limit: i64,
    ) -> Result<Vec<StationOrderBook>, MarketDbError> {
        if limit <= 0 {
            return Ok(Vec::new());
        }

        let language_fallbacks = language_fallbacks(language);
        let rows = sqlx::query_as::<_, StationOrderBookRecord>(
            "WITH latest_run AS (
                SELECT sync_run_id, COALESCE(completed_at, started_at) AS synced_at
                FROM evetools_catalog.market_sync_runs
                WHERE region_id = $1 AND status = 'success'
                ORDER BY completed_at DESC NULLS LAST, sync_run_id DESC
                LIMIT 1
             ),
             station_orders AS (
                SELECT
                    lr.synced_at,
                    o.sync_run_id,
                    o.region_id,
                    o.station_id,
                    o.type_id,
                    o.is_buy_order,
                    o.price,
                    o.volume_remain
                FROM evetools_catalog.market_order_snapshots o
                JOIN latest_run lr ON lr.sync_run_id = o.sync_run_id
                WHERE o.region_id = $1 AND o.station_id = $2
             ),
             best_prices AS (
                SELECT
                    sync_run_id,
                    region_id,
                    station_id,
                    type_id,
                    synced_at,
                    MAX(price) FILTER (WHERE is_buy_order) AS best_bid,
                    MIN(price) FILTER (WHERE NOT is_buy_order) AS best_ask,
                    SUM(volume_remain)::BIGINT AS visible_volume
                FROM station_orders
                GROUP BY sync_run_id, region_id, station_id, type_id, synced_at
                HAVING
                    MAX(price) FILTER (WHERE is_buy_order) IS NOT NULL
                    AND MIN(price) FILTER (WHERE NOT is_buy_order) IS NOT NULL
             ),
             books AS (
                SELECT
                    bp.sync_run_id,
                    bp.region_id,
                    bp.station_id,
                    bp.type_id,
                    bp.synced_at,
                    bp.best_bid,
                    bp.best_ask,
                    COALESCE(SUM(CASE
                        WHEN o.is_buy_order AND o.price = bp.best_bid THEN o.volume_remain
                        ELSE 0
                    END), 0)::BIGINT AS top_buy_depth,
                    COALESCE(SUM(CASE
                        WHEN NOT o.is_buy_order AND o.price = bp.best_ask THEN o.volume_remain
                        ELSE 0
                    END), 0)::BIGINT AS top_sell_depth,
                    bp.visible_volume
                FROM best_prices bp
                JOIN station_orders o
                    ON o.sync_run_id = bp.sync_run_id
                    AND o.station_id = bp.station_id
                    AND o.type_id = bp.type_id
                GROUP BY
                    bp.sync_run_id,
                    bp.region_id,
                    bp.station_id,
                    bp.type_id,
                    bp.synced_at,
                    bp.best_bid,
                    bp.best_ask,
                    bp.visible_volume
             )
             SELECT
                b.sync_run_id,
                b.region_id,
                b.station_id,
                b.type_id,
                COALESCE(
                    (SELECT l.name
                     FROM evetools_catalog.inventory_type_localizations l
                     WHERE l.type_id = b.type_id AND l.name IS NOT NULL
                     ORDER BY COALESCE(array_position($3::text[], l.language), 2147483647), l.language
                     LIMIT 1),
                    t.name_en,
                    t.name_zh
                ) AS display_name,
                b.best_bid,
                b.best_ask,
                b.top_buy_depth,
                b.top_sell_depth,
                b.visible_volume,
                b.synced_at
             FROM books b
             LEFT JOIN evetools_catalog.inventory_types t ON t.type_id = b.type_id
             ORDER BY
                ((b.best_ask - b.best_bid) / NULLIF(b.best_bid, 0)) DESC NULLS LAST,
                b.visible_volume DESC,
                b.type_id
             LIMIT $4",
        )
        .persistent(false)
        .bind(region_id)
        .bind(station_id)
        .bind(language_fallbacks)
        .bind(limit.min(MAX_STATION_ORDER_BOOK_LIMIT))
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(station_order_book_from_record)
            .collect())
    }

    pub async fn latest_station_order_book(
        &self,
        region_id: i32,
        station_id: i64,
        type_id: i32,
        language: &str,
    ) -> Result<Option<StationOrderBook>, MarketDbError> {
        let language_fallbacks = language_fallbacks(language);
        let row = sqlx::query_as::<_, StationOrderBookRecord>(
            "WITH latest_run AS (
                SELECT sync_run_id, COALESCE(completed_at, started_at) AS synced_at
                FROM evetools_catalog.market_sync_runs
                WHERE region_id = $1 AND status = 'success'
                ORDER BY completed_at DESC NULLS LAST, sync_run_id DESC
                LIMIT 1
             ),
             station_orders AS (
                SELECT
                    lr.synced_at,
                    o.sync_run_id,
                    o.region_id,
                    o.station_id,
                    o.type_id,
                    o.is_buy_order,
                    o.price,
                    o.volume_remain
                FROM evetools_catalog.market_order_snapshots o
                JOIN latest_run lr ON lr.sync_run_id = o.sync_run_id
                WHERE o.region_id = $1 AND o.station_id = $2 AND o.type_id = $3
             ),
             best_prices AS (
                SELECT
                    sync_run_id,
                    region_id,
                    station_id,
                    type_id,
                    synced_at,
                    MAX(price) FILTER (WHERE is_buy_order) AS best_bid,
                    MIN(price) FILTER (WHERE NOT is_buy_order) AS best_ask,
                    SUM(volume_remain)::BIGINT AS visible_volume
                FROM station_orders
                GROUP BY sync_run_id, region_id, station_id, type_id, synced_at
                HAVING
                    MAX(price) FILTER (WHERE is_buy_order) IS NOT NULL
                    AND MIN(price) FILTER (WHERE NOT is_buy_order) IS NOT NULL
             ),
             books AS (
                SELECT
                    bp.sync_run_id,
                    bp.region_id,
                    bp.station_id,
                    bp.type_id,
                    bp.synced_at,
                    bp.best_bid,
                    bp.best_ask,
                    COALESCE(SUM(CASE
                        WHEN o.is_buy_order AND o.price = bp.best_bid THEN o.volume_remain
                        ELSE 0
                    END), 0)::BIGINT AS top_buy_depth,
                    COALESCE(SUM(CASE
                        WHEN NOT o.is_buy_order AND o.price = bp.best_ask THEN o.volume_remain
                        ELSE 0
                    END), 0)::BIGINT AS top_sell_depth,
                    bp.visible_volume
                FROM best_prices bp
                JOIN station_orders o
                    ON o.sync_run_id = bp.sync_run_id
                    AND o.station_id = bp.station_id
                    AND o.type_id = bp.type_id
                GROUP BY
                    bp.sync_run_id,
                    bp.region_id,
                    bp.station_id,
                    bp.type_id,
                    bp.synced_at,
                    bp.best_bid,
                    bp.best_ask,
                    bp.visible_volume
             )
             SELECT
                b.sync_run_id,
                b.region_id,
                b.station_id,
                b.type_id,
                COALESCE(
                    (SELECT l.name
                     FROM evetools_catalog.inventory_type_localizations l
                     WHERE l.type_id = b.type_id AND l.name IS NOT NULL
                     ORDER BY COALESCE(array_position($4::text[], l.language), 2147483647), l.language
                     LIMIT 1),
                    t.name_en,
                    t.name_zh
                ) AS display_name,
                b.best_bid,
                b.best_ask,
                b.top_buy_depth,
                b.top_sell_depth,
                b.visible_volume,
                b.synced_at
             FROM books b
             LEFT JOIN evetools_catalog.inventory_types t ON t.type_id = b.type_id",
        )
        .persistent(false)
        .bind(region_id)
        .bind(station_id)
        .bind(type_id)
        .bind(language_fallbacks)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(station_order_book_from_record))
    }

    pub async fn sync_health_at(
        &self,
        now: DateTime<Utc>,
    ) -> Result<MarketSyncHealthReport, MarketDbError> {
        let hubs = self.list_enabled_trade_hubs().await?;
        let mut health = Vec::with_capacity(hubs.len());
        for hub in hubs {
            health.push(self.sync_health_for_hub(now, hub).await?);
        }

        Ok(MarketSyncHealthReport {
            generated_at: now.to_rfc3339(),
            hubs: health,
        })
    }

    async fn latest_successful_sync_run(
        &self,
        region_id: i32,
    ) -> Result<Option<i64>, MarketDbError> {
        let sync_run_id = sqlx::query_scalar(
            "SELECT sync_run_id
             FROM evetools_catalog.market_sync_runs
             WHERE region_id = $1 AND status = 'success'
             ORDER BY completed_at DESC NULLS LAST, sync_run_id DESC
             LIMIT 1",
        )
        .persistent(false)
        .bind(region_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(sync_run_id)
    }

    async fn expire_region_leases(&self, region_id: i32) -> Result<(), MarketDbError> {
        sqlx::query(
            "UPDATE evetools_catalog.market_sync_runs
             SET status = 'expired',
                 completed_at = NOW(),
                 completed_reason = 'lease_expired',
                 duration_ms = (EXTRACT(EPOCH FROM (NOW() - started_at)) * 1000)::BIGINT
             WHERE region_id = $1
               AND status IN ('leased', 'running')
               AND (
                    lease_expires_at < NOW()
                    OR (
                        lease_expires_at IS NULL
                        AND started_at < NOW() - INTERVAL '20 minutes'
                    )
               )",
        )
        .persistent(false)
        .bind(region_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn active_sync_run(&self, region_id: i32) -> Result<Option<i64>, MarketDbError> {
        let sync_run_id = sqlx::query_scalar(
            "SELECT sync_run_id
             FROM evetools_catalog.market_sync_runs
             WHERE region_id = $1 AND status IN ('leased', 'running')
             ORDER BY started_at DESC, sync_run_id DESC
             LIMIT 1",
        )
        .persistent(false)
        .bind(region_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(sync_run_id)
    }

    async fn ensure_sync_run_can_write_snapshots(
        &self,
        sync_run_id: i64,
    ) -> Result<(), MarketDbError> {
        let can_write = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                SELECT 1
                FROM evetools_catalog.market_sync_runs
                WHERE sync_run_id = $1
                  AND status = 'running'
                  AND (lease_expires_at IS NULL OR lease_expires_at >= NOW())
             )",
        )
        .persistent(false)
        .bind(sync_run_id)
        .fetch_one(&self.pool)
        .await?;

        if !can_write {
            return Err(MarketDbError::InactiveSyncRun { sync_run_id });
        }
        Ok(())
    }

    async fn sync_health_for_hub(
        &self,
        now: DateTime<Utc>,
        hub: TradeHub,
    ) -> Result<TradeHubSyncHealth, MarketDbError> {
        let active = self
            .active_non_expired_sync_attempt(hub.region_id, now)
            .await?;
        let latest_success = self.latest_successful_sync_attempt(hub.region_id).await?;
        let latest_attempt = self.latest_sync_attempt(hub.region_id).await?;
        let consecutive_failures = self
            .consecutive_failed_sync_attempts(
                hub.region_id,
                latest_success.as_ref().map(|row| row.1),
            )
            .await?;

        let mut status = classify_sync_health(&hub.hub_id, now, latest_success.as_ref());
        if active.is_some() {
            status = MarketSyncHealthStatus::Syncing;
        } else if latest_attempt
            .as_ref()
            .is_some_and(|attempt| attempt.1 == "failed")
            && !matches!(status, MarketSyncHealthStatus::Fresh)
        {
            status = MarketSyncHealthStatus::Degraded;
        }

        let latest_attempt = active.or(latest_attempt);
        let age_seconds = latest_success
            .as_ref()
            .map(|success| (now - success.1).num_seconds().max(0));

        Ok(TradeHubSyncHealth {
            hub_id: hub.hub_id,
            display_name: hub.display_name,
            region_id: hub.region_id,
            station_id: hub.station_id,
            status,
            latest_success_sync_run_id: latest_success.as_ref().map(|row| row.0),
            latest_success_completed_at: latest_success.as_ref().map(|row| row.1.to_rfc3339()),
            latest_attempt_sync_run_id: latest_attempt.as_ref().map(|row| row.0),
            latest_attempt_status: latest_attempt.as_ref().map(|row| row.1.clone()),
            latest_attempt_error: latest_attempt.as_ref().and_then(|row| row.2.clone()),
            age_seconds,
            order_count: latest_success.as_ref().and_then(|row| row.2),
            consecutive_failures,
        })
    }

    async fn active_non_expired_sync_attempt(
        &self,
        region_id: i32,
        now: DateTime<Utc>,
    ) -> Result<Option<SyncAttemptRecord>, MarketDbError> {
        let row = sqlx::query_as::<_, SyncAttemptRecord>(
            "SELECT sync_run_id, status, error_summary
             FROM evetools_catalog.market_sync_runs
             WHERE region_id = $1
               AND status IN ('leased', 'running')
               AND (
                    lease_expires_at >= $2
                    OR (
                        lease_expires_at IS NULL
                        AND started_at >= $2 - INTERVAL '20 minutes'
                    )
               )
             ORDER BY started_at DESC, sync_run_id DESC
             LIMIT 1",
        )
        .persistent(false)
        .bind(region_id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn latest_successful_sync_attempt(
        &self,
        region_id: i32,
    ) -> Result<Option<SyncSuccessRecord>, MarketDbError> {
        let row = sqlx::query_as::<_, SyncSuccessRecord>(
            "SELECT sync_run_id, completed_at, order_count
             FROM evetools_catalog.market_sync_runs
             WHERE region_id = $1 AND status = 'success' AND completed_at IS NOT NULL
             ORDER BY completed_at DESC, sync_run_id DESC
             LIMIT 1",
        )
        .persistent(false)
        .bind(region_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn latest_sync_attempt(
        &self,
        region_id: i32,
    ) -> Result<Option<SyncAttemptRecord>, MarketDbError> {
        let row = sqlx::query_as::<_, SyncAttemptRecord>(
            "SELECT sync_run_id, status, error_summary
             FROM evetools_catalog.market_sync_runs
             WHERE region_id = $1
             ORDER BY started_at DESC, sync_run_id DESC
             LIMIT 1",
        )
        .persistent(false)
        .bind(region_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn consecutive_failed_sync_attempts(
        &self,
        region_id: i32,
        latest_success_completed_at: Option<DateTime<Utc>>,
    ) -> Result<i64, MarketDbError> {
        let count = match latest_success_completed_at {
            Some(completed_at) => {
                sqlx::query_scalar(
                    "SELECT COUNT(*)::BIGINT
                     FROM evetools_catalog.market_sync_runs
                     WHERE region_id = $1
                       AND status = 'failed'
                       AND completed_at > $2",
                )
                .persistent(false)
                .bind(region_id)
                .bind(completed_at)
                .fetch_one(&self.pool)
                .await?
            }
            None => {
                sqlx::query_scalar(
                    "SELECT COUNT(*)::BIGINT
                     FROM evetools_catalog.market_sync_runs
                     WHERE region_id = $1 AND status = 'failed'",
                )
                .persistent(false)
                .bind(region_id)
                .fetch_one(&self.pool)
                .await?
            }
        };
        Ok(count)
    }
}

type TradeHubRecord = (String, String, i32, i32, i64, bool, i32);
type SyncAttemptRecord = (i64, String, Option<String>);
type SyncSuccessRecord = (i64, DateTime<Utc>, Option<i64>);
type MarketOrderSnapshotRecord = (
    i64,
    i32,
    i64,
    i32,
    i64,
    bool,
    f64,
    i64,
    i64,
    String,
    i32,
    i32,
    String,
    i32,
);
type StationOrderBookRecord = (
    i64,
    i32,
    i64,
    i32,
    Option<String>,
    f64,
    f64,
    i64,
    i64,
    i64,
    DateTime<Utc>,
);

fn trade_hub_from_record(row: TradeHubRecord) -> TradeHub {
    TradeHub {
        hub_id: row.0,
        display_name: row.1,
        region_id: row.2,
        system_id: row.3,
        station_id: row.4,
        enabled: row.5,
        sort_order: row.6,
    }
}

fn market_order_from_record(row: MarketOrderSnapshotRecord) -> MarketOrderSnapshot {
    MarketOrderSnapshot {
        sync_run_id: row.0,
        region_id: row.1,
        station_id: row.2,
        type_id: row.3,
        order_id: row.4,
        is_buy_order: row.5,
        price: row.6,
        volume_remain: row.7,
        volume_total: row.8,
        issued: row.9,
        duration: row.10,
        min_volume: row.11,
        order_range: row.12,
        system_id: row.13,
    }
}

fn station_order_book_from_record(row: StationOrderBookRecord) -> StationOrderBook {
    StationOrderBook {
        sync_run_id: row.0,
        region_id: row.1,
        station_id: row.2,
        type_id: row.3,
        display_name: row.4.unwrap_or_else(|| format!("Type {}", row.3)),
        best_bid: row.5,
        best_ask: row.6,
        top_buy_depth: row.7,
        top_sell_depth: row.8,
        visible_volume: row.9,
        last_synced_at: row.10.to_rfc3339(),
    }
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

fn classify_sync_health(
    hub_id: &str,
    now: DateTime<Utc>,
    latest_success: Option<&SyncSuccessRecord>,
) -> MarketSyncHealthStatus {
    let Some((_, completed_at, _)) = latest_success else {
        return MarketSyncHealthStatus::Missing;
    };

    let age_seconds = (now - *completed_at).num_seconds().max(0);
    let (fresh_seconds, stale_seconds) = sync_health_thresholds(hub_id);
    if age_seconds <= fresh_seconds {
        MarketSyncHealthStatus::Fresh
    } else if age_seconds <= stale_seconds {
        MarketSyncHealthStatus::Stale
    } else {
        MarketSyncHealthStatus::Expired
    }
}

fn sync_health_thresholds(hub_id: &str) -> (i64, i64) {
    match hub_id {
        "jita" => (15 * 60, 30 * 60),
        "amarr" => (30 * 60, 60 * 60),
        _ => (45 * 60, 90 * 60),
    }
}
