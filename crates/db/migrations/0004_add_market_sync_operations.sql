ALTER TABLE evetools_catalog.market_sync_runs
    ADD COLUMN IF NOT EXISTS lease_owner TEXT,
    ADD COLUMN IF NOT EXISTS lease_expires_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS started_by TEXT,
    ADD COLUMN IF NOT EXISTS attempt INTEGER NOT NULL DEFAULT 1,
    ADD COLUMN IF NOT EXISTS duration_ms BIGINT,
    ADD COLUMN IF NOT EXISTS completed_reason TEXT;

WITH ranked_active_runs AS (
    SELECT
        sync_run_id,
        ROW_NUMBER() OVER (
            PARTITION BY region_id
            ORDER BY started_at DESC, sync_run_id DESC
        ) AS active_rank
    FROM evetools_catalog.market_sync_runs
    WHERE status IN ('leased', 'running')
)
UPDATE evetools_catalog.market_sync_runs run
SET status = 'expired',
    completed_at = COALESCE(run.completed_at, NOW()),
    completed_reason = COALESCE(run.completed_reason, 'superseded_before_lease_index'),
    duration_ms = COALESCE(
        run.duration_ms,
        (EXTRACT(EPOCH FROM (COALESCE(run.completed_at, NOW()) - run.started_at)) * 1000)::BIGINT
    )
FROM ranked_active_runs ranked
WHERE run.sync_run_id = ranked.sync_run_id
  AND ranked.active_rank > 1;

CREATE UNIQUE INDEX IF NOT EXISTS idx_evetools_market_sync_runs_one_active_region
    ON evetools_catalog.market_sync_runs(region_id)
    WHERE status IN ('leased', 'running');
