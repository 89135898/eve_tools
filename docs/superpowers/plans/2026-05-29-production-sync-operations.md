# Production Sync Operations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add production-grade public market sync leases, sync health, readiness APIs, and scheduler-friendly worker behavior without adding alert delivery integrations.

**Architecture:** Extend the existing Supabase/Postgres `market_sync_runs` model with lease metadata and operational state. Keep `evetools-worker` as the writer and `evetools-http-api` as the read-only health surface; desktop clients continue to call only the hosted API.

**Tech Stack:** Rust 1.82, `sqlx` Postgres migrations, `chrono`, `serde`, `axum`, existing `evetools-db`, `evetools-api`, `evetools-worker`, and `evetools-http-api` crates.

---

## File Map

- Modify `crates/db/migrations/0004_add_market_sync_operations.sql`: add lease/run metadata columns and active-run partial unique index.
- Modify `crates/db/src/schema.rs`: assert migration 4 is registered and migration SQL contains the operations fields.
- Modify `crates/db/src/market.rs`: add lease acquisition, stale lease expiration, skipped runs, sync health projection, and readiness support.
- Modify `crates/db/tests/market_repository.rs`: integration tests for leases, stale lease expiration, run outcomes, and health statuses.
- Modify `crates/worker/src/lib.rs`: parse production CLI flags, run all default regions, use repository leases, support skip/max-age, JSON summaries, and already-running success semantics.
- Modify `crates/worker/src/bin/sync-public-market-region.rs`: print JSON or plain text and preserve exit behavior.
- Modify `crates/worker/tests/public_market_sync.rs`: tests for JSON output, all-region config, lease conflict, stale skip, and failed ESI recording.
- Modify `crates/api/src/lib.rs`: expose readiness and sync-health read methods.
- Modify `crates/api/tests/read_api.rs`: verify readiness and sync-health views.
- Modify `crates/http-api/src/lib.rs`: add `GET /ready` and `GET /sync-health`.
- Modify `crates/http-api/tests/read_http_api.rs`: verify new HTTP routes.
- Modify `README.md`: document production scheduling, worker options, readiness, sync health, and secret boundaries.

---

### Task 1: Add Market Sync Operations Migration

**Files:**
- Create: `crates/db/migrations/0004_add_market_sync_operations.sql`
- Modify: `crates/db/src/schema.rs`

- [ ] **Step 1: Write the failing schema test**

In `crates/db/src/schema.rs`, update the migration list test to expect version 4 and add field/index assertions:

```rust
assert_eq!(
    migrations,
    vec![
        (1, "create catalog schema"),
        (2, "add catalog localizations"),
        (3, "add market sync tables"),
        (4, "add market sync operations")
    ]
);
```

Add this test:

```rust
#[test]
fn adds_market_sync_operation_metadata() {
    assert_schema_contains("ALTER TABLE evetools_catalog.market_sync_runs");
    assert_schema_contains("lease_owner TEXT");
    assert_schema_contains("lease_expires_at TIMESTAMPTZ");
    assert_schema_contains("started_by TEXT");
    assert_schema_contains("attempt INTEGER NOT NULL DEFAULT 1");
    assert_schema_contains("duration_ms BIGINT");
    assert_schema_contains("completed_reason TEXT");
    assert_schema_contains("idx_evetools_market_sync_runs_one_active_region");
    assert_schema_contains("WHERE status IN ('leased', 'running')");
}
```

- [ ] **Step 2: Run the schema unit test and verify it fails**

Run:

```bash
cargo test -p evetools-db schema::tests::adds_market_sync_operation_metadata -- --nocapture
```

Expected: FAIL because migration 4 and the asserted SQL do not exist.

- [ ] **Step 3: Add the migration**

Create `crates/db/migrations/0004_add_market_sync_operations.sql`:

```sql
ALTER TABLE evetools_catalog.market_sync_runs
    ADD COLUMN IF NOT EXISTS lease_owner TEXT,
    ADD COLUMN IF NOT EXISTS lease_expires_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS started_by TEXT,
    ADD COLUMN IF NOT EXISTS attempt INTEGER NOT NULL DEFAULT 1,
    ADD COLUMN IF NOT EXISTS duration_ms BIGINT,
    ADD COLUMN IF NOT EXISTS completed_reason TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_evetools_market_sync_runs_one_active_region
    ON evetools_catalog.market_sync_runs(region_id)
    WHERE status IN ('leased', 'running');
```

- [ ] **Step 4: Run migration/schema tests**

Run:

```bash
cargo test -p evetools-db schema::tests::catalog_migrations_are_versioned_by_feature_slice schema::tests::adds_market_sync_operation_metadata -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/db/migrations/0004_add_market_sync_operations.sql crates/db/src/schema.rs
git commit -m "feat: add market sync operations migration"
```

---

### Task 2: Add Repository Lease and Health Model

**Files:**
- Modify: `crates/db/src/market.rs`
- Modify: `crates/db/tests/market_repository.rs`

- [ ] **Step 1: Write failing repository tests**

Add these imports to `crates/db/tests/market_repository.rs`:

```rust
use chrono::{Duration, Utc};
use evetools_db::{MarketSyncHealthStatus, MarketSyncStartStatus};
```

Change `prepare_market_repository()` to return both pool and repository:

```rust
async fn prepare_market_repository() -> Option<(sqlx::PgPool, MarketRepository)> {
    let url = match guarded_database_url_from_env() {
        Ok(Some(url)) => url,
        Ok(None) => {
            eprintln!("skipping Postgres test: EVETOOLS_TEST_DATABASE_URL is not set");
            return None;
        }
        Err(error) => panic!("{error}"),
    };
    let pool = connect_pool(&url).await.unwrap();
    reset_evetools_catalog_schema(&pool).await.unwrap();
    migrate_catalog_schema(&pool).await.unwrap();
    let repository = MarketRepository::new(pool.clone());
    Some((pool, repository))
}
```

Update existing call sites from:

```rust
let Some(repository) = prepare_market_repository().await else {
    return;
};
```

to:

```rust
let Some((_pool, repository)) = prepare_market_repository().await else {
    return;
};
```

Add tests:

```rust
#[tokio::test]
async fn leases_reject_concurrent_region_syncs_and_expire_stale_runs() {
    let _guard = POSTGRES_TEST_LOCK.lock().await;
    let Some((pool, repository)) = prepare_market_repository().await else {
        return;
    };

    let lease = repository
        .try_start_sync_run(
            10000002,
            "public-esi",
            "test-worker",
            "lease-a",
            Duration::minutes(20),
        )
        .await
        .unwrap();
    assert!(matches!(lease.status, MarketSyncStartStatus::Started));
    let sync_run_id = lease.sync_run_id.unwrap();

    let blocked = repository
        .try_start_sync_run(
            10000002,
            "public-esi",
            "test-worker",
            "lease-b",
            Duration::minutes(20),
        )
        .await
        .unwrap();
    assert_eq!(blocked.status, MarketSyncStartStatus::AlreadyRunning);
    assert_eq!(blocked.sync_run_id, Some(sync_run_id));

    sqlx::query(
        "UPDATE evetools_catalog.market_sync_runs
         SET lease_expires_at = NOW() - INTERVAL '1 minute'
         WHERE sync_run_id = $1",
    )
    .bind(sync_run_id)
    .execute(&pool)
    .await
    .unwrap();

    let replacement = repository
        .try_start_sync_run(
            10000002,
            "public-esi",
            "test-worker",
            "lease-c",
            Duration::minutes(20),
        )
        .await
        .unwrap();
    assert_eq!(replacement.status, MarketSyncStartStatus::Started);
    assert_ne!(replacement.sync_run_id, Some(sync_run_id));

    let expired_status: String = sqlx::query_scalar(
        "SELECT status FROM evetools_catalog.market_sync_runs WHERE sync_run_id = $1",
    )
    .bind(sync_run_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(expired_status, "expired");
}

#[tokio::test]
async fn sync_health_classifies_missing_fresh_stale_expired_and_degraded() {
    let _guard = POSTGRES_TEST_LOCK.lock().await;
    let Some((pool, repository)) = prepare_market_repository().await else {
        return;
    };

    repository.upsert_trade_hubs(&[jita_hub()]).await.unwrap();
    let now = Utc::now();

    let missing = repository.sync_health_at(now).await.unwrap();
    assert_eq!(missing.hubs[0].status, MarketSyncHealthStatus::Missing);

    let fresh_run = repository.start_sync_run(10000002, "public-esi").await.unwrap();
    repository.complete_sync_run(fresh_run, 1, 1).await.unwrap();
    let fresh = repository.sync_health_at(now).await.unwrap();
    assert_eq!(fresh.hubs[0].status, MarketSyncHealthStatus::Fresh);

    sqlx::query(
        "UPDATE evetools_catalog.market_sync_runs
         SET completed_at = NOW() - INTERVAL '40 minutes'
         WHERE sync_run_id = $1",
    )
    .bind(fresh_run)
    .execute(&pool)
    .await
    .unwrap();
    let stale = repository.sync_health_at(now).await.unwrap();
    assert_eq!(stale.hubs[0].status, MarketSyncHealthStatus::Stale);

    sqlx::query(
        "UPDATE evetools_catalog.market_sync_runs
         SET completed_at = NOW() - INTERVAL '2 hours'
         WHERE sync_run_id = $1",
    )
    .bind(fresh_run)
    .execute(&pool)
    .await
    .unwrap();
    let expired = repository.sync_health_at(now).await.unwrap();
    assert_eq!(expired.hubs[0].status, MarketSyncHealthStatus::Expired);

    let failed_run = repository.start_sync_run(10000002, "public-esi").await.unwrap();
    repository.fail_sync_run(failed_run, "esi unavailable").await.unwrap();
    let degraded = repository.sync_health_at(now).await.unwrap();
    assert_eq!(degraded.hubs[0].status, MarketSyncHealthStatus::Degraded);
    assert_eq!(degraded.hubs[0].consecutive_failures, 1);
}
```

- [ ] **Step 2: Run repository tests and verify they fail**

Run:

```bash
cargo test -p evetools-db --test market_repository -- --nocapture
```

Expected: FAIL with unresolved `MarketSyncHealthStatus`, `MarketSyncStartStatus`, `try_start_sync_run`, and `sync_health_at`.

- [ ] **Step 3: Add repository types**

In `crates/db/src/market.rs`, add:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketSyncStartStatus {
    Started,
    AlreadyRunning,
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
```

- [ ] **Step 4: Implement lease methods**

Add methods to `impl MarketRepository`:

```rust
pub async fn try_start_sync_run(
    &self,
    region_id: i32,
    source: &str,
    started_by: &str,
    lease_owner: &str,
    lease_ttl: chrono::Duration,
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
    sqlx::query(
        "UPDATE evetools_catalog.market_sync_runs
         SET status = 'running'
         WHERE sync_run_id = $1",
    )
    .persistent(false)
    .bind(sync_run_id)
    .execute(&self.pool)
    .await?;
    Ok(())
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
           AND lease_expires_at < NOW()",
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
```

- [ ] **Step 5: Update run completion methods**

Change `complete_sync_run()` and `fail_sync_run()` updates to include `duration_ms`, `completed_reason`, and clear lease fields:

```sql
duration_ms = (EXTRACT(EPOCH FROM (NOW() - started_at)) * 1000)::BIGINT,
completed_reason = 'completed',
lease_owner = NULL,
lease_expires_at = NULL
```

For failure use `completed_reason = 'failed'`.

- [ ] **Step 6: Implement `sync_health_at`**

Add this method:

```rust
pub async fn sync_health_at(
    &self,
    now: DateTime<Utc>,
) -> Result<MarketSyncHealthReport, MarketDbError> {
    let hubs = self.list_enabled_trade_hubs().await?;
    let mut rows = Vec::new();
    for hub in hubs {
        rows.push(self.sync_health_for_hub(now, hub).await?);
    }
    Ok(MarketSyncHealthReport {
        generated_at: now.to_rfc3339(),
        hubs: rows,
    })
}
```

Implement `sync_health_for_hub()` using three queries per hub: latest success, latest attempt, and consecutive failures after latest success. Classify Jita with 15/30 minute thresholds and all other hubs with 45/90 minute thresholds, except Amarr with 30/60.

- [ ] **Step 7: Run repository tests**

Run:

```bash
cargo test -p evetools-db --test market_repository -- --nocapture
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/db/src/market.rs crates/db/tests/market_repository.rs
git commit -m "feat: add market sync leases and health"
```

---

### Task 3: Refactor Worker Sync Flow and CLI

**Files:**
- Modify: `crates/worker/src/lib.rs`
- Modify: `crates/worker/src/bin/sync-public-market-region.rs`
- Modify: `crates/worker/tests/public_market_sync.rs`

- [ ] **Step 1: Write failing worker config tests**

In `crates/worker/src/lib.rs`, add tests:

```rust
#[test]
fn cli_config_accepts_production_flags() {
    let config = PublicMarketSyncCliConfig::from_args_and_env(
        [
            "--all-default-regions",
            "--started-by",
            "github-actions",
            "--lease-ttl-seconds",
            "900",
            "--max-age-seconds=600",
            "--json",
        ],
        |name| match name {
            "EVETOOLS_DATABASE_URL" => Some("postgres://example".to_string()),
            _ => None,
        },
    )
    .unwrap();

    assert!(config.all_default_regions);
    assert_eq!(config.region_id, None);
    assert_eq!(config.started_by, "github-actions");
    assert_eq!(config.lease_ttl_seconds, 900);
    assert_eq!(config.max_age_seconds, Some(600));
    assert!(config.json);
}

#[test]
fn public_market_sync_summary_serializes_json_without_secrets() {
    let summary = PublicMarketSyncSummary {
        sync_run_id: Some(42),
        region_id: 10000002,
        status: "success".to_string(),
        order_count: 5,
        page_count: 1,
        message: "synced".to_string(),
    };

    let json = serde_json::to_string(&summary).unwrap();
    assert!(json.contains("\"region_id\":10000002"));
    assert!(!json.contains("postgres"));
}
```

- [ ] **Step 2: Run worker unit tests and verify failure**

Run:

```bash
cargo test -p evetools-worker cli_config_accepts_production_flags public_market_sync_summary_serializes_json_without_secrets -- --nocapture
```

Expected: FAIL because config fields and summary fields are missing.

- [ ] **Step 3: Update worker config and summary types**

Change `PublicMarketSyncCliConfig`:

```rust
pub struct PublicMarketSyncCliConfig {
    pub database_url: String,
    pub esi_base_url: Option<String>,
    pub region_id: Option<i32>,
    pub all_default_regions: bool,
    pub started_by: String,
    pub lease_ttl_seconds: i64,
    pub max_age_seconds: Option<i64>,
    pub json: bool,
}
```

Change `PublicMarketSyncSummary`:

```rust
pub struct PublicMarketSyncSummary {
    pub sync_run_id: Option<i64>,
    pub region_id: i32,
    pub status: String,
    pub order_count: i64,
    pub page_count: i32,
    pub message: String,
}
```

- [ ] **Step 4: Extend argument parser**

Support:

```rust
"--all-default-regions" => all_default_regions = true,
"--started-by" => started_by = next_value("--started-by")?,
value if value.starts_with("--started-by=") => started_by = value.trim_start_matches("--started-by=").to_string(),
"--lease-ttl-seconds" => lease_ttl_seconds = parse_i64(next_value("--lease-ttl-seconds")?)?,
value if value.starts_with("--lease-ttl-seconds=") => lease_ttl_seconds = parse_i64(value.trim_start_matches("--lease-ttl-seconds="))?,
"--max-age-seconds" => max_age_seconds = Some(parse_i64(next_value("--max-age-seconds")?)?),
value if value.starts_with("--max-age-seconds=") => max_age_seconds = Some(parse_i64(value.trim_start_matches("--max-age-seconds="))?),
"--json" => json = true,
```

Keep `--region-id` and positional region ID support. Reject `--all-default-regions` combined with explicit region ID.

- [ ] **Step 5: Use repository leases in sync flow**

In `sync_public_market_region_orders`, call:

```rust
let lease = repository
    .try_start_sync_run(
        region_id,
        "public-esi",
        started_by,
        &default_lease_owner(),
        chrono::Duration::seconds(lease_ttl_seconds),
    )
    .await?;
if lease.status == evetools_db::MarketSyncStartStatus::AlreadyRunning {
    return Ok(PublicMarketSyncSummary {
        sync_run_id: lease.sync_run_id,
        region_id,
        status: "already-running".to_string(),
        order_count: 0,
        page_count: 0,
        message: lease.message,
    });
}
let sync_run_id = lease.sync_run_id.expect("started lease should have sync_run_id");
repository.mark_sync_run_running(sync_run_id).await?;
```

Use the returned `sync_run_id` for snapshots. On success return:

```rust
PublicMarketSyncSummary {
    sync_run_id: Some(sync_run_id),
    region_id,
    status: "success".to_string(),
    order_count: snapshots.len() as i64,
    page_count: 0,
    message: "synced".to_string(),
}
```

- [ ] **Step 6: Add all-region runner**

Make `run_public_market_region_sync(config)` return `Vec<PublicMarketSyncSummary>`. If `all_default_regions` is true, collect unique region IDs from `default_trade_hubs()` and sync each sequentially. Otherwise sync `config.region_id.unwrap_or(DEFAULT_PUBLIC_MARKET_REGION_ID)`.

- [ ] **Step 7: Update CLI binary output**

In `crates/worker/src/bin/sync-public-market-region.rs`:

```rust
let config = PublicMarketSyncCliConfig::from_env_and_args(std::env::args().skip(1))?;
let json = config.json;
let summaries = run_public_market_region_sync(config).await?;
for summary in summaries {
    if json {
        println!("{}", serde_json::to_string(&summary).expect("summary should serialize"));
    } else {
        println!("{}", format_public_market_sync_summary(&summary));
    }
}
```

- [ ] **Step 8: Run worker tests**

Run:

```bash
cargo test -p evetools-worker -- --nocapture
cargo test -p evetools-worker --test public_market_sync -- --nocapture
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/worker/src/lib.rs crates/worker/src/bin/sync-public-market-region.rs crates/worker/tests/public_market_sync.rs
git commit -m "feat: harden public market sync worker"
```

---

### Task 4: Add Read API Readiness and Sync Health

**Files:**
- Modify: `crates/api/src/lib.rs`
- Modify: `crates/api/tests/read_api.rs`

- [ ] **Step 1: Write failing API tests**

In `crates/api/tests/read_api.rs`, after the existing hub checks, add:

```rust
let readiness = api.readiness().await.unwrap();
assert_eq!(readiness.status, "ready");
assert_eq!(readiness.database, "ok");
assert_eq!(readiness.catalog, "ok");

let health = api.sync_health().await.unwrap();
assert!(!health.generated_at.is_empty());
assert_eq!(health.hubs[0].hub_id, "jita");
assert_eq!(health.hubs[0].status, evetools_db::MarketSyncHealthStatus::Fresh);
```

- [ ] **Step 2: Run API test and verify failure**

Run:

```bash
cargo test -p evetools-api --test read_api read_api_exposes_catalog_and_market_queries -- --nocapture
```

Expected: FAIL because `readiness` and `sync_health` do not exist.

- [ ] **Step 3: Add API view type**

In `crates/api/src/lib.rs`, add:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadinessView {
    pub status: String,
    pub database: String,
    pub catalog: String,
    pub market_sync: String,
}
```

Re-export sync health:

```rust
pub use evetools_db::{
    CatalogStatus as CatalogStatusView, InventoryTypeView as InventoryTypeApiView,
    MarketOrderSnapshot as MarketOrderView, MarketSyncHealthReport as SyncHealthView,
    TradeHub as TradeHubView,
};
```

- [ ] **Step 4: Keep pool in read API**

Change `EveToolsReadApi`:

```rust
pub struct EveToolsReadApi {
    pool: PgPool,
    catalog: CatalogRepository,
    market: MarketRepository,
}
```

Update `from_pool`:

```rust
pub fn from_pool(pool: PgPool) -> Self {
    Self {
        pool: pool.clone(),
        catalog: CatalogRepository::new(pool.clone()),
        market: MarketRepository::new(pool),
    }
}
```

- [ ] **Step 5: Implement methods**

Add:

```rust
pub async fn readiness(&self) -> Result<ReadinessView, ApiError> {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .persistent(false)
        .fetch_one(&self.pool)
        .await?;
    let catalog = self.catalog.latest_status().await?;
    let health = self.market.sync_health_at(chrono::Utc::now()).await?;
    let market_sync = if health
        .hubs
        .iter()
        .any(|hub| matches!(hub.status, evetools_db::MarketSyncHealthStatus::Fresh | evetools_db::MarketSyncHealthStatus::Stale | evetools_db::MarketSyncHealthStatus::Syncing))
    {
        "ok"
    } else {
        "degraded"
    };

    Ok(ReadinessView {
        status: "ready".to_string(),
        database: "ok".to_string(),
        catalog: if catalog.status == "success" { "ok" } else { "degraded" }.to_string(),
        market_sync: market_sync.to_string(),
    })
}

pub async fn sync_health(&self) -> Result<SyncHealthView, ApiError> {
    Ok(self.market.sync_health_at(chrono::Utc::now()).await?)
}
```

Add `chrono.workspace = true` to `crates/api/Cargo.toml` if needed.

- [ ] **Step 6: Run API tests**

Run:

```bash
cargo test -p evetools-api --test read_api -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/lib.rs crates/api/Cargo.toml crates/api/tests/read_api.rs
git commit -m "feat: expose sync readiness from read api"
```

---

### Task 5: Add HTTP `/ready` and `/sync-health`

**Files:**
- Modify: `crates/http-api/src/lib.rs`
- Modify: `crates/http-api/tests/read_http_api.rs`

- [ ] **Step 1: Write failing HTTP tests**

In `crates/http-api/tests/read_http_api.rs`, after `/health` assertion add:

```rust
let ready = router.clone().oneshot(request("/ready")).await.unwrap();
assert_eq!(ready.status(), 200);
let ready = json_body(ready).await;
assert_eq!(ready["status"], "ready");
assert_eq!(ready["database"], "ok");

let sync_health = router
    .clone()
    .oneshot(request("/sync-health"))
    .await
    .unwrap();
assert_eq!(sync_health.status(), 200);
let sync_health = json_body(sync_health).await;
assert_eq!(sync_health["hubs"].as_array().unwrap()[0]["hub_id"], "jita");
```

- [ ] **Step 2: Run HTTP test and verify failure**

Run:

```bash
cargo test -p evetools-http-api --test read_http_api read_http_api_exposes_health_hubs_and_selection_candidates -- --nocapture
```

Expected: FAIL because `/ready` and `/sync-health` are missing.

- [ ] **Step 3: Add routes**

In `build_router()` add:

```rust
.route("/ready", get(readiness))
.route("/sync-health", get(sync_health))
```

Add handlers:

```rust
async fn readiness(State(api): State<EveToolsReadApi>) -> Result<Response, HttpApiError> {
    Ok(Json(api.readiness().await?).into_response())
}

async fn sync_health(State(api): State<EveToolsReadApi>) -> Result<Response, HttpApiError> {
    Ok(Json(api.sync_health().await?).into_response())
}
```

- [ ] **Step 4: Run HTTP tests**

Run:

```bash
cargo test -p evetools-http-api --test read_http_api -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/http-api/src/lib.rs crates/http-api/tests/read_http_api.rs
git commit -m "feat: add sync health http routes"
```

---

### Task 6: Document Production Scheduling Without Alerting

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Add documentation**

Add a section under public market sync:

```markdown
### 生产同步任务

生产环境中，桌面端不运行同步任务。同步由服务端 scheduler 触发 `evetools-worker`：

```bash
export EVETOOLS_DATABASE_URL="<worker-postgres-url-with-sslmode-require>"
cargo run -p evetools-worker --bin sync-public-market-region -- \
  --all-default-regions \
  --started-by production-scheduler \
  --lease-ttl-seconds 1200 \
  --max-age-seconds 600 \
  --json
```

`--all-default-regions` 会按顺序同步 Jita、Amarr、Dodixie、Rens、Hek。Worker 会为每个 region 获取数据库 lease；如果另一个 worker 已经在同步同一 region，本次运行会返回 `already-running` 并以成功退出，避免 scheduler 因正常锁竞争误报失败。

本阶段不接入监控报警平台。生产健康状态通过 HTTP API 暴露：

- `GET /health`：进程存活。
- `GET /ready`：数据库、catalog 和市场同步可用性。
- `GET /sync-health`：每个 trade hub 的最新同步时间、状态、失败信息和连续失败次数。
```

- [ ] **Step 2: Run markdown and workspace checks**

Run:

```bash
git diff --check
cargo test --workspace
pnpm --filter @evetools/desktop typecheck
```

Expected: all pass.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: document production market sync operations"
```

---

### Task 7: Final Verification

**Files:**
- Verify all modified files.

- [ ] **Step 1: Run full verification**

Run:

```bash
cargo test --workspace
pnpm --filter @evetools/desktop typecheck
git status --short --branch
```

Expected:

- `cargo test --workspace` exits 0.
- `pnpm --filter @evetools/desktop typecheck` exits 0.
- `git status --short --branch` shows the branch state and no uncommitted changes after final commits.

- [ ] **Step 2: Report completion state**

Report:

- commits created
- tests run
- any skipped Postgres integration tests if `EVETOOLS_TEST_DATABASE_URL` was not set
- whether `master` is ahead of `origin/master`

