# Production Sync Operations Design

Date: 2026-05-29

## Purpose

Define the production-grade task and health model for EveTools public market synchronization.

The goal is to make market data refreshes safe to run unattended in a hosted environment while preserving the current architecture: desktop clients call the hosted HTTP API, the API reads Supabase/Postgres, and worker/admin processes are the only components allowed to write market or catalog data.

Monitoring and alert delivery are intentionally out of scope for this slice. The system should expose enough health data for a later alerting layer, but it should not integrate Slack, email, PagerDuty, Grafana, Better Stack, or any cloud-specific alerting service yet.

## Confirmed Scope

- Keep `evetools-http-api` as a read-oriented hosted service.
- Keep `evetools-worker` as the public market synchronization entrypoint.
- Extend the database schema with production task metadata instead of relying only on ad hoc CLI output.
- Make public market sync single-flight per region so overlapping schedulers or manual retries cannot write competing snapshots.
- Record enough run metadata to support operations, debugging, and future alert rules.
- Add API readiness and sync-health endpoints.
- Document production scheduling and secret boundaries.

## Out Of Scope

- Alert delivery integrations.
- A web operations dashboard.
- Kubernetes-specific deployment manifests.
- Queue systems such as Kafka, RabbitMQ, or Redis.
- Authenticated EVE SSO and character order synchronization.
- Replacing Supabase/Postgres.
- Moving worker scheduling into the desktop app.

## Current Baseline

The current implementation already has these production-relevant pieces:

- `trade_hubs`
- `market_sync_runs`
- `market_order_snapshots`
- `sync-public-market-region` CLI
- `EveToolsReadApi`
- hosted HTTP routes for market lookup, station orders, trade hubs, and selection candidates
- desktop usage of `EVETOOLS_API_BASE_URL`

The current gap is not the data product path. The gap is operational control: sync runs can be started manually or by a scheduler, but there is no durable region lock, no stale-data health classification, no readiness endpoint, no consecutive failure view, and no standard production scheduling contract.

## Recommended Architecture

Use a three-layer production operations model:

```text
External scheduler
  |
  | invokes worker for one region or all configured regions
  v
evetools-worker
  |
  | acquires per-region lease, runs ESI sync, writes run outcome
  v
Supabase/Postgres
  |
  | exposes latest run and health projection
  v
evetools-http-api
  |
  | /ready and /sync-health for clients and later alerting
  v
desktop app
```

The scheduler can initially be GitHub Actions cron, Cloud Run Jobs, Fly Machines, systemd timers, Render cron jobs, or another platform. The code must not depend on a specific scheduler. The scheduler contract is simply: run the worker command with server-side secrets.

## Database Design

Keep `market_sync_runs` as the canonical run history table and add columns instead of replacing it:

- `lease_owner TEXT`
- `lease_expires_at TIMESTAMPTZ`
- `started_by TEXT`
- `attempt INTEGER NOT NULL DEFAULT 1`
- `duration_ms BIGINT`
- `completed_reason TEXT`

Add a partial unique index to prevent concurrent active runs for the same region:

```sql
CREATE UNIQUE INDEX IF NOT EXISTS idx_evetools_market_sync_runs_one_active_region
    ON evetools_catalog.market_sync_runs(region_id)
    WHERE status IN ('running', 'leased');
```

Use explicit statuses:

- `leased`: worker reserved the region but has not started ESI fetch yet.
- `running`: worker is actively syncing the region.
- `success`: sync completed and snapshots were replaced.
- `failed`: sync failed after the worker started.
- `expired`: previous lease exceeded its timeout and a later worker marked it stale before taking over.
- `skipped`: worker intentionally did not sync because the latest success is still fresh enough.

Do not add a separate `sync_jobs` table in this slice. The current product only has a fixed set of region sync jobs, and the active scheduling source can be external. A separate job definition table becomes useful when users can create or pause arbitrary recurring jobs, which is not needed yet.

## Lease and Concurrency Policy

Each worker run uses a generated `lease_owner`, such as `hostname:pid:uuid`.

For a requested `region_id`, the worker must:

1. Upsert default trade hubs.
2. Mark expired active runs for the region as `expired` when `lease_expires_at < NOW()`.
3. Insert a new `market_sync_runs` row with status `leased`, `lease_owner`, `lease_expires_at`, `started_by`, and `source`.
4. Rely on the partial unique index to reject concurrent leases.
5. If a lease conflict occurs, return a typed `AlreadyRunning` result and exit successfully with a clear message.
6. Move the run to `running` immediately before ESI fetch.
7. On success, replace snapshots and mark `success`.
8. On failure, mark `failed` with a truncated error summary.

The default lease TTL should be 20 minutes for region order sync. This is long enough for slow ESI or network conditions but short enough to recover from killed processes.

## Freshness Policy

Use per-hub freshness thresholds because Jita is more important than smaller hubs:

| Hub | Region | Fresh For | Stale After |
| --- | ---: | ---: | ---: |
| Jita | 10000002 | 15 minutes | 30 minutes |
| Amarr | 10000043 | 30 minutes | 60 minutes |
| Dodixie | 10000032 | 45 minutes | 90 minutes |
| Rens | 10000030 | 45 minutes | 90 minutes |
| Hek | 10000042 | 45 minutes | 90 minutes |

Health classification:

- `fresh`: latest successful sync age is within `Fresh For`.
- `stale`: latest successful sync age is above `Fresh For` but within `Stale After`.
- `expired`: latest successful sync age is above `Stale After`.
- `missing`: there is no successful sync for the hub region.
- `syncing`: there is an active non-expired run for the hub region.
- `degraded`: latest completed run failed and the latest success is stale or missing.

The HTTP API should expose these classifications without deciding how alerts are delivered.

## API Design

Keep `/health` as a cheap liveness route:

```json
{ "status": "ok" }
```

Add `/ready` for dependency readiness:

```json
{
  "status": "ready",
  "database": "ok",
  "catalog": "ok",
  "market_sync": "degraded"
}
```

`/ready` should return HTTP 200 when the API process can serve requests and the database is reachable. It may include `market_sync: degraded` without returning 500, because stale market data should not make catalog search unavailable.

Add `/sync-health`:

```json
{
  "generated_at": "2026-05-29T12:00:00Z",
  "hubs": [
    {
      "hub_id": "jita",
      "display_name": "Jita",
      "region_id": 10000002,
      "station_id": 60003760,
      "status": "fresh",
      "latest_success_sync_run_id": 123,
      "latest_success_completed_at": "2026-05-29T11:55:00Z",
      "latest_attempt_sync_run_id": 124,
      "latest_attempt_status": "success",
      "latest_attempt_error": null,
      "age_seconds": 300,
      "order_count": 982341,
      "consecutive_failures": 0
    }
  ]
}
```

The API returns all enabled trade hubs ordered by `sort_order`.

## Worker CLI Design

Keep the existing command:

```bash
cargo run -p evetools-worker --bin sync-public-market-region -- --region-id 10000002
```

Add production-oriented options:

- `--all-default-regions`: sync Jita, Amarr, Dodixie, Rens, and Hek sequentially.
- `--started-by <value>`: record scheduler identity, for example `github-actions`, `manual`, or `cloud-run`.
- `--lease-ttl-seconds <seconds>`: override default lease TTL for tests or incident recovery.
- `--max-age-seconds <seconds>`: skip sync when latest success is fresh enough.
- `--json`: output one JSON summary per region.

Exit behavior:

- Return exit code 0 when sync succeeds.
- Return exit code 0 when sync is skipped because data is fresh.
- Return exit code 0 when another worker already owns the region lease.
- Return exit code 1 for configuration errors, migration errors, database errors, or ESI failures.

This keeps external schedulers simple: non-zero means the scheduled job failed. Lease conflict is not a failure because it means another worker is already doing the work.

## Structured Logs

Worker logs should be line-delimited JSON when `--json` is used. Required fields:

- `event`
- `region_id`
- `sync_run_id`
- `status`
- `lease_owner`
- `started_at`
- `completed_at`
- `duration_ms`
- `page_count`
- `order_count`
- `error_summary`

Plain text output can remain for local development.

## Secret and Permission Boundaries

Use separate database credentials by environment role:

- HTTP API: read catalog, read market snapshots, read sync health.
- Worker: read catalog, write market sync runs, write order snapshots, upsert trade hubs.
- Catalog admin: write catalog tables and run SDE import.

The desktop app must only receive `EVETOOLS_API_BASE_URL`. It must never receive `EVETOOLS_DATABASE_URL`.

## Testing Strategy

Repository integration tests should cover:

- acquiring a region lease inserts a leased run
- a second active lease for the same region is rejected
- expired active leases are marked `expired`
- successful completion records duration, page count, and order count
- failed completion records a truncated error summary
- sync health returns `missing`, `fresh`, `stale`, `expired`, and `degraded`

Worker tests should cover:

- `--all-default-regions` runs each default region once
- fresh data with `--max-age-seconds` returns skipped
- lease conflict returns an already-running summary and exit success
- ESI failure records failed run and returns error
- JSON output is valid and redacts secrets

HTTP API tests should cover:

- `/ready` returns database and market sync readiness fields
- `/sync-health` returns enabled hubs in sort order
- stale or missing market sync does not break `/catalog/status`

Desktop tests do not need to change for this slice unless the UI later shows sync health.

## Implementation Order

1. Add migration for run lease metadata and the active-run unique index.
2. Add repository methods for lease acquisition, stale lease expiration, completion, failure, skip, and health projection.
3. Refactor worker sync flow to use the lease API.
4. Add CLI options and JSON output.
5. Add read API methods for readiness and sync health.
6. Add HTTP routes `/ready` and `/sync-health`.
7. Update README with production scheduling examples and role-based secret boundaries.

This order keeps each step testable and avoids introducing scheduler-specific code before the database and API contracts are stable.

