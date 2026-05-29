#[cfg(test)]
use sqlx::migrate::Migration;
use sqlx::{
    migrate::MigrateError,
    postgres::{PgConnectOptions, PgPoolOptions},
    Executor, PgPool,
};
use std::str::FromStr;

const CATALOG_SESSION_SETUP_SQL: &str = "SET lock_timeout = '15s'; SET statement_timeout = '300s'";
static CATALOG_MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

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

pub async fn migrate_catalog_schema(pool: &PgPool) -> Result<(), MigrateError> {
    CATALOG_MIGRATOR.run(pool).await
}

#[cfg(test)]
fn catalog_migrations() -> &'static [Migration] {
    CATALOG_MIGRATOR.migrations.as_ref()
}

#[cfg(test)]
fn catalog_migration_sql() -> String {
    catalog_migrations()
        .iter()
        .map(|migration| migration.sql.as_ref())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_migrations_are_versioned_by_feature_slice() {
        let migrations: Vec<_> = catalog_migrations()
            .iter()
            .map(|migration| (migration.version, migration.description.as_ref()))
            .collect();

        assert_eq!(
            migrations,
            vec![
                (1, "create catalog schema"),
                (2, "add catalog localizations"),
                (3, "add market sync tables"),
                (4, "add market sync operations"),
                (5, "add authenticated order monitor")
            ]
        );
    }

    #[test]
    fn creates_parent_catalog_tables_before_child_tables() {
        let categories =
            migration_sql_index("CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_categories");
        let groups =
            migration_sql_index("CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_groups");
        let market_groups =
            migration_sql_index("CREATE TABLE IF NOT EXISTS evetools_catalog.market_groups");
        let types =
            migration_sql_index("CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_types");

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
    fn adds_market_sync_operation_metadata() {
        assert_schema_contains("ALTER TABLE evetools_catalog.market_sync_runs");
        assert_schema_contains("lease_owner TEXT");
        assert_schema_contains("lease_expires_at TIMESTAMPTZ");
        assert_schema_contains("started_by TEXT");
        assert_schema_contains("attempt INTEGER NOT NULL DEFAULT 1");
        assert_schema_contains("duration_ms BIGINT");
        assert_schema_contains("completed_reason TEXT");
        assert_schema_contains("superseded_before_lease_index");
        assert_schema_contains("ROW_NUMBER() OVER");
        assert_schema_contains("idx_evetools_market_sync_runs_one_active_region");
        assert_schema_contains("WHERE status IN ('leased', 'running')");
    }

    #[test]
    fn adds_authenticated_order_monitor_tables() {
        assert_schema_contains("CREATE TABLE IF NOT EXISTS evetools_catalog.characters");
        assert_schema_contains("CREATE TABLE IF NOT EXISTS evetools_catalog.character_auth_tokens");
        assert_schema_contains("CREATE TABLE IF NOT EXISTS evetools_catalog.character_order_sync_runs");
        assert_schema_contains("CREATE TABLE IF NOT EXISTS evetools_catalog.character_order_snapshots");
        assert_schema_contains("PRIMARY KEY (character_id)");
        assert_schema_contains("refresh_token TEXT NOT NULL");
        assert_schema_contains("scopes TEXT[] NOT NULL");
        assert_schema_contains("PRIMARY KEY (sync_run_id, order_id)");
        assert_schema_contains("idx_evetools_character_order_snapshots_character_type");
    }

    #[test]
    fn connection_session_setup_bounds_lock_and_statement_waits() {
        assert!(CATALOG_SESSION_SETUP_SQL.contains("lock_timeout"));
        assert!(CATALOG_SESSION_SETUP_SQL.contains("statement_timeout"));
    }

    fn statement_index(needle: &str) -> usize {
        catalog_migration_sql()
            .find(needle)
            .expect("schema statement should exist")
    }

    fn assert_schema_contains(needle: &str) {
        assert!(
            catalog_migration_sql().contains(needle),
            "schema statements should contain {needle}"
        );
    }

    fn migration_sql_index(needle: &str) -> usize {
        statement_index(needle)
    }
}
