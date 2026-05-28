use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, QueryBuilder};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MarketDbError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
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

    pub async fn complete_sync_run(
        &self,
        sync_run_id: i64,
        page_count: i32,
        order_count: i64,
    ) -> Result<(), MarketDbError> {
        sqlx::query(
            "UPDATE evetools_catalog.market_sync_runs
             SET completed_at = NOW(), status = 'success',
                 page_count = $1, order_count = $2, error_summary = NULL
             WHERE sync_run_id = $3",
        )
        .persistent(false)
        .bind(page_count)
        .bind(order_count)
        .bind(sync_run_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn fail_sync_run(
        &self,
        sync_run_id: i64,
        error_summary: &str,
    ) -> Result<(), MarketDbError> {
        let error_summary: String = error_summary.chars().take(1_000).collect();
        sqlx::query(
            "UPDATE evetools_catalog.market_sync_runs
             SET completed_at = NOW(), status = 'failed', error_summary = $1
             WHERE sync_run_id = $2",
        )
        .persistent(false)
        .bind(error_summary)
        .bind(sync_run_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn replace_order_snapshots(
        &self,
        sync_run_id: i64,
        orders: &[MarketOrderSnapshotInput],
    ) -> Result<(), MarketDbError> {
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
}

type TradeHubRecord = (String, String, i32, i32, i64, bool, i32);
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
