use sqlx::{postgres::PgPoolOptions, PgPool};

pub async fn connect_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
}

pub async fn migrate_catalog_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    for statement in CATALOG_SCHEMA_STATEMENTS {
        sqlx::query(statement).execute(pool).await?;
    }
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
