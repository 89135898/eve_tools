CREATE TABLE IF NOT EXISTS evetools_catalog.trade_hubs (
    hub_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    region_id INTEGER NOT NULL,
    system_id INTEGER NOT NULL,
    station_id BIGINT NOT NULL,
    enabled BOOLEAN NOT NULL,
    sort_order INTEGER NOT NULL,
    PRIMARY KEY (hub_id)
);

CREATE TABLE IF NOT EXISTS evetools_catalog.market_sync_runs (
    sync_run_id BIGSERIAL PRIMARY KEY,
    region_id INTEGER NOT NULL,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    status TEXT NOT NULL,
    page_count INTEGER NOT NULL DEFAULT 0,
    order_count BIGINT NOT NULL DEFAULT 0,
    error_summary TEXT,
    source TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS evetools_catalog.market_order_snapshots (
    sync_run_id BIGINT NOT NULL REFERENCES evetools_catalog.market_sync_runs(sync_run_id) ON DELETE CASCADE,
    region_id INTEGER NOT NULL,
    station_id BIGINT NOT NULL,
    type_id INTEGER NOT NULL,
    order_id BIGINT NOT NULL,
    is_buy_order BOOLEAN NOT NULL,
    price DOUBLE PRECISION NOT NULL,
    volume_remain BIGINT NOT NULL,
    volume_total BIGINT NOT NULL,
    issued TEXT NOT NULL,
    duration INTEGER NOT NULL,
    min_volume INTEGER NOT NULL,
    order_range TEXT NOT NULL,
    system_id INTEGER NOT NULL,
    PRIMARY KEY (sync_run_id, order_id)
);

CREATE INDEX IF NOT EXISTS idx_evetools_market_sync_runs_region_status_completed
    ON evetools_catalog.market_sync_runs(region_id, status, completed_at DESC);

CREATE INDEX IF NOT EXISTS idx_evetools_market_orders_station_type
    ON evetools_catalog.market_order_snapshots(region_id, station_id, type_id);
