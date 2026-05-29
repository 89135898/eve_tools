CREATE TABLE IF NOT EXISTS evetools_catalog.characters (
    character_id BIGINT NOT NULL,
    character_name TEXT NOT NULL,
    owner_hash TEXT,
    last_login_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (character_id)
);

CREATE TABLE IF NOT EXISTS evetools_catalog.character_auth_tokens (
    character_id BIGINT NOT NULL
        REFERENCES evetools_catalog.characters(character_id)
        ON DELETE CASCADE,
    refresh_token TEXT NOT NULL,
    access_token TEXT,
    access_token_expires_at TIMESTAMPTZ,
    scopes TEXT[] NOT NULL,
    token_type TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (character_id)
);

CREATE TABLE IF NOT EXISTS evetools_catalog.character_order_sync_runs (
    sync_run_id BIGSERIAL PRIMARY KEY,
    character_id BIGINT NOT NULL
        REFERENCES evetools_catalog.characters(character_id)
        ON DELETE CASCADE,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    status TEXT NOT NULL,
    order_count BIGINT,
    error_summary TEXT
);

CREATE INDEX IF NOT EXISTS idx_evetools_character_order_sync_runs_character_status_completed
    ON evetools_catalog.character_order_sync_runs(character_id, status, completed_at DESC);

CREATE TABLE IF NOT EXISTS evetools_catalog.character_order_snapshots (
    sync_run_id BIGINT NOT NULL
        REFERENCES evetools_catalog.character_order_sync_runs(sync_run_id)
        ON DELETE CASCADE,
    character_id BIGINT NOT NULL
        REFERENCES evetools_catalog.characters(character_id)
        ON DELETE CASCADE,
    order_id BIGINT NOT NULL,
    type_id INTEGER NOT NULL,
    region_id INTEGER NOT NULL,
    location_id BIGINT NOT NULL,
    is_buy_order BOOLEAN NOT NULL,
    price DOUBLE PRECISION NOT NULL,
    volume_remain BIGINT NOT NULL,
    volume_total BIGINT NOT NULL,
    issued TEXT NOT NULL,
    duration INTEGER NOT NULL,
    min_volume INTEGER,
    order_range TEXT NOT NULL,
    is_corporation BOOLEAN NOT NULL,
    escrow DOUBLE PRECISION,
    PRIMARY KEY (sync_run_id, order_id)
);

CREATE INDEX IF NOT EXISTS idx_evetools_character_order_snapshots_character_type
    ON evetools_catalog.character_order_snapshots(character_id, type_id, location_id);
