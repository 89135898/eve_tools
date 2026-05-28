CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_category_localizations (
    category_id INTEGER NOT NULL REFERENCES evetools_catalog.inventory_categories(category_id) ON DELETE CASCADE,
    language TEXT NOT NULL,
    name TEXT,
    updated_import_id BIGINT NOT NULL REFERENCES evetools_catalog.sde_imports(import_id),
    PRIMARY KEY (category_id, language)
);

CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_group_localizations (
    group_id INTEGER NOT NULL REFERENCES evetools_catalog.inventory_groups(group_id) ON DELETE CASCADE,
    language TEXT NOT NULL,
    name TEXT,
    updated_import_id BIGINT NOT NULL REFERENCES evetools_catalog.sde_imports(import_id),
    PRIMARY KEY (group_id, language)
);

CREATE TABLE IF NOT EXISTS evetools_catalog.market_group_localizations (
    market_group_id INTEGER NOT NULL REFERENCES evetools_catalog.market_groups(market_group_id) ON DELETE CASCADE,
    language TEXT NOT NULL,
    name TEXT,
    description TEXT,
    updated_import_id BIGINT NOT NULL REFERENCES evetools_catalog.sde_imports(import_id),
    PRIMARY KEY (market_group_id, language)
);

CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_type_localizations (
    type_id INTEGER NOT NULL REFERENCES evetools_catalog.inventory_types(type_id) ON DELETE CASCADE,
    language TEXT NOT NULL,
    name TEXT,
    description TEXT,
    updated_import_id BIGINT NOT NULL REFERENCES evetools_catalog.sde_imports(import_id),
    PRIMARY KEY (type_id, language)
);

CREATE INDEX IF NOT EXISTS idx_evetools_inventory_type_localizations_language_name
    ON evetools_catalog.inventory_type_localizations(language, name);

CREATE INDEX IF NOT EXISTS idx_evetools_inventory_group_localizations_language_name
    ON evetools_catalog.inventory_group_localizations(language, name);

CREATE INDEX IF NOT EXISTS idx_evetools_inventory_category_localizations_language_name
    ON evetools_catalog.inventory_category_localizations(language, name);

CREATE INDEX IF NOT EXISTS idx_evetools_market_group_localizations_language_name
    ON evetools_catalog.market_group_localizations(language, name);
