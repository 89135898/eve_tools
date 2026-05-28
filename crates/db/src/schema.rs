use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    Executor, PgPool,
};
use std::str::FromStr;

const CATALOG_MIGRATION_LOCK_KEY: i64 = 912_345_678_901_234_568;
const CATALOG_SESSION_SETUP_SQL: &str = "SET lock_timeout = '15s'; SET statement_timeout = '300s'";

pub async fn connect_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let options = PgConnectOptions::from_str(database_url)?.statement_cache_capacity(0);
    PgPoolOptions::new()
        .max_connections(5)
        .after_connect(|connection, _metadata| {
            Box::pin(async move {
                connection.execute(CATALOG_SESSION_SETUP_SQL).await?;
                Ok(())
            })
        })
        .connect_with(options)
        .await
}

pub async fn migrate_catalog_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .persistent(false)
        .bind(CATALOG_MIGRATION_LOCK_KEY)
        .execute(&mut *tx)
        .await?;

    for statement in CATALOG_SCHEMA_STATEMENTS {
        sqlx::query(statement)
            .persistent(false)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(())
}

const CATALOG_SCHEMA_STATEMENTS: &[&str] = &[
    r#"CREATE SCHEMA IF NOT EXISTS evetools_catalog"#,
    r#"CREATE TABLE IF NOT EXISTS evetools_catalog.sde_imports (
        import_id BIGSERIAL PRIMARY KEY,
        build_number INTEGER,
        release_date TEXT,
        source_url TEXT NOT NULL,
        started_at TIMESTAMPTZ NOT NULL,
        completed_at TIMESTAMPTZ,
        status TEXT NOT NULL,
        error_summary TEXT,
        type_count BIGINT NOT NULL DEFAULT 0,
        group_count BIGINT NOT NULL DEFAULT 0,
        category_count BIGINT NOT NULL DEFAULT 0,
        market_group_count BIGINT NOT NULL DEFAULT 0
    )"#,
    r#"CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_categories (
        category_id INTEGER PRIMARY KEY,
        published BOOLEAN NOT NULL,
        name_en TEXT,
        name_zh TEXT,
        raw_name_json JSONB NOT NULL,
        updated_import_id BIGINT NOT NULL REFERENCES evetools_catalog.sde_imports(import_id)
    )"#,
    r#"CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_groups (
        group_id INTEGER PRIMARY KEY,
        category_id INTEGER NOT NULL,
        published BOOLEAN NOT NULL,
        name_en TEXT,
        name_zh TEXT,
        raw_name_json JSONB NOT NULL,
        updated_import_id BIGINT NOT NULL REFERENCES evetools_catalog.sde_imports(import_id)
    )"#,
    r#"CREATE TABLE IF NOT EXISTS evetools_catalog.market_groups (
        market_group_id INTEGER PRIMARY KEY,
        parent_group_id INTEGER,
        name_en TEXT,
        name_zh TEXT,
        description_en TEXT,
        description_zh TEXT,
        raw_name_json JSONB NOT NULL,
        raw_description_json JSONB,
        updated_import_id BIGINT NOT NULL REFERENCES evetools_catalog.sde_imports(import_id)
    )"#,
    r#"CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_types (
        type_id INTEGER PRIMARY KEY,
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
        updated_import_id BIGINT NOT NULL REFERENCES evetools_catalog.sde_imports(import_id)
    )"#,
    r#"CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_category_localizations (
        category_id INTEGER NOT NULL REFERENCES evetools_catalog.inventory_categories(category_id) ON DELETE CASCADE,
        language TEXT NOT NULL,
        name TEXT,
        updated_import_id BIGINT NOT NULL REFERENCES evetools_catalog.sde_imports(import_id),
        PRIMARY KEY (category_id, language)
    )"#,
    r#"CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_group_localizations (
        group_id INTEGER NOT NULL REFERENCES evetools_catalog.inventory_groups(group_id) ON DELETE CASCADE,
        language TEXT NOT NULL,
        name TEXT,
        updated_import_id BIGINT NOT NULL REFERENCES evetools_catalog.sde_imports(import_id),
        PRIMARY KEY (group_id, language)
    )"#,
    r#"CREATE TABLE IF NOT EXISTS evetools_catalog.market_group_localizations (
        market_group_id INTEGER NOT NULL REFERENCES evetools_catalog.market_groups(market_group_id) ON DELETE CASCADE,
        language TEXT NOT NULL,
        name TEXT,
        description TEXT,
        updated_import_id BIGINT NOT NULL REFERENCES evetools_catalog.sde_imports(import_id),
        PRIMARY KEY (market_group_id, language)
    )"#,
    r#"CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_type_localizations (
        type_id INTEGER NOT NULL REFERENCES evetools_catalog.inventory_types(type_id) ON DELETE CASCADE,
        language TEXT NOT NULL,
        name TEXT,
        description TEXT,
        updated_import_id BIGINT NOT NULL REFERENCES evetools_catalog.sde_imports(import_id),
        PRIMARY KEY (type_id, language)
    )"#,
    r#"CREATE TABLE IF NOT EXISTS evetools_catalog.trade_hubs (
        hub_id TEXT NOT NULL,
        display_name TEXT NOT NULL,
        region_id INTEGER NOT NULL,
        system_id INTEGER NOT NULL,
        station_id BIGINT NOT NULL,
        enabled BOOLEAN NOT NULL,
        sort_order INTEGER NOT NULL,
        PRIMARY KEY (hub_id)
    )"#,
    r#"CREATE TABLE IF NOT EXISTS evetools_catalog.market_sync_runs (
        sync_run_id BIGSERIAL PRIMARY KEY,
        region_id INTEGER NOT NULL,
        started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
        completed_at TIMESTAMPTZ,
        status TEXT NOT NULL,
        page_count INTEGER NOT NULL DEFAULT 0,
        order_count BIGINT NOT NULL DEFAULT 0,
        error_summary TEXT,
        source TEXT NOT NULL
    )"#,
    r#"CREATE TABLE IF NOT EXISTS evetools_catalog.market_order_snapshots (
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
    )"#,
    r#"DO $$
    BEGIN
        ALTER TABLE evetools_catalog.inventory_groups
            ADD CONSTRAINT fk_inventory_groups_category_id
            FOREIGN KEY (category_id)
            REFERENCES evetools_catalog.inventory_categories(category_id)
            DEFERRABLE INITIALLY DEFERRED;
    EXCEPTION WHEN duplicate_object THEN NULL;
    END $$"#,
    r#"DO $$
    BEGIN
        ALTER TABLE evetools_catalog.market_groups
            ADD CONSTRAINT fk_market_groups_parent_group_id
            FOREIGN KEY (parent_group_id)
            REFERENCES evetools_catalog.market_groups(market_group_id)
            DEFERRABLE INITIALLY DEFERRED;
    EXCEPTION WHEN duplicate_object THEN NULL;
    END $$"#,
    r#"DO $$
    BEGIN
        ALTER TABLE evetools_catalog.inventory_types
            ADD CONSTRAINT fk_inventory_types_group_id
            FOREIGN KEY (group_id)
            REFERENCES evetools_catalog.inventory_groups(group_id)
            DEFERRABLE INITIALLY DEFERRED;
    EXCEPTION WHEN duplicate_object THEN NULL;
    END $$"#,
    r#"DO $$
    BEGIN
        ALTER TABLE evetools_catalog.inventory_types
            ADD CONSTRAINT fk_inventory_types_market_group_id
            FOREIGN KEY (market_group_id)
            REFERENCES evetools_catalog.market_groups(market_group_id)
            DEFERRABLE INITIALLY DEFERRED;
    EXCEPTION WHEN duplicate_object THEN NULL;
    END $$"#,
    r#"CREATE INDEX IF NOT EXISTS idx_evetools_inventory_types_name_en
        ON evetools_catalog.inventory_types(name_en)"#,
    r#"CREATE INDEX IF NOT EXISTS idx_evetools_inventory_types_name_zh
        ON evetools_catalog.inventory_types(name_zh)"#,
    r#"CREATE INDEX IF NOT EXISTS idx_evetools_inventory_types_market_group
        ON evetools_catalog.inventory_types(market_group_id)"#,
    r#"CREATE INDEX IF NOT EXISTS idx_evetools_inventory_type_localizations_language_name
        ON evetools_catalog.inventory_type_localizations(language, name)"#,
    r#"CREATE INDEX IF NOT EXISTS idx_evetools_inventory_group_localizations_language_name
        ON evetools_catalog.inventory_group_localizations(language, name)"#,
    r#"CREATE INDEX IF NOT EXISTS idx_evetools_inventory_category_localizations_language_name
        ON evetools_catalog.inventory_category_localizations(language, name)"#,
    r#"CREATE INDEX IF NOT EXISTS idx_evetools_market_group_localizations_language_name
        ON evetools_catalog.market_group_localizations(language, name)"#,
    r#"CREATE INDEX IF NOT EXISTS idx_evetools_market_sync_runs_region_status_completed
        ON evetools_catalog.market_sync_runs(region_id, status, completed_at DESC)"#,
    r#"CREATE INDEX IF NOT EXISTS idx_evetools_market_orders_station_type
        ON evetools_catalog.market_order_snapshots(region_id, station_id, type_id)"#,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_parent_catalog_tables_before_child_tables() {
        let categories =
            statement_index("CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_categories");
        let groups =
            statement_index("CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_groups");
        let market_groups =
            statement_index("CREATE TABLE IF NOT EXISTS evetools_catalog.market_groups");
        let types = statement_index("CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_types");

        assert!(categories < groups);
        assert!(groups < market_groups);
        assert!(market_groups < types);
    }

    #[test]
    fn adds_core_sde_foreign_keys_as_deferred_constraints() {
        assert_schema_contains("fk_inventory_groups_category_id");
        assert_schema_contains("FOREIGN KEY (category_id)");
        assert_schema_contains("REFERENCES evetools_catalog.inventory_categories(category_id)");
        assert_schema_contains("fk_inventory_types_group_id");
        assert_schema_contains("FOREIGN KEY (group_id)");
        assert_schema_contains("REFERENCES evetools_catalog.inventory_groups(group_id)");
        assert_schema_contains("fk_inventory_types_market_group_id");
        assert_schema_contains("FOREIGN KEY (market_group_id)");
        assert_schema_contains("REFERENCES evetools_catalog.market_groups(market_group_id)");
        assert_schema_contains("fk_market_groups_parent_group_id");
        assert_schema_contains("FOREIGN KEY (parent_group_id)");
        assert_schema_contains("REFERENCES evetools_catalog.market_groups(market_group_id)");
        assert_schema_contains("DEFERRABLE INITIALLY DEFERRED");
        assert_schema_contains("EXCEPTION WHEN duplicate_object THEN NULL");
    }

    #[test]
    fn creates_standard_localization_tables_and_indexes() {
        assert_schema_contains(
            "CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_type_localizations",
        );
        assert_schema_contains(
            "CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_group_localizations",
        );
        assert_schema_contains(
            "CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_category_localizations",
        );
        assert_schema_contains(
            "CREATE TABLE IF NOT EXISTS evetools_catalog.market_group_localizations",
        );
        assert_schema_contains("PRIMARY KEY (type_id, language)");
        assert_schema_contains("PRIMARY KEY (group_id, language)");
        assert_schema_contains("PRIMARY KEY (category_id, language)");
        assert_schema_contains("PRIMARY KEY (market_group_id, language)");
        assert_schema_contains("idx_evetools_inventory_type_localizations_language_name");
    }

    #[test]
    fn creates_public_market_sync_tables_and_indexes() {
        assert_schema_contains("CREATE TABLE IF NOT EXISTS evetools_catalog.trade_hubs");
        assert_schema_contains("CREATE TABLE IF NOT EXISTS evetools_catalog.market_sync_runs");
        assert_schema_contains(
            "CREATE TABLE IF NOT EXISTS evetools_catalog.market_order_snapshots",
        );
        assert_schema_contains("PRIMARY KEY (hub_id)");
        assert_schema_contains("PRIMARY KEY (sync_run_id, order_id)");
        assert_schema_contains("idx_evetools_market_orders_station_type");
        assert_schema_contains("idx_evetools_market_sync_runs_region_status_completed");
    }

    #[test]
    fn connection_session_setup_bounds_lock_and_statement_waits() {
        assert!(CATALOG_SESSION_SETUP_SQL.contains("lock_timeout"));
        assert!(CATALOG_SESSION_SETUP_SQL.contains("statement_timeout"));
    }

    fn statement_index(needle: &str) -> usize {
        CATALOG_SCHEMA_STATEMENTS
            .iter()
            .position(|statement| statement.contains(needle))
            .expect("schema statement should exist")
    }

    fn assert_schema_contains(needle: &str) {
        assert!(
            CATALOG_SCHEMA_STATEMENTS
                .iter()
                .any(|statement| statement.contains(needle)),
            "schema statements should contain {needle}"
        );
    }
}
