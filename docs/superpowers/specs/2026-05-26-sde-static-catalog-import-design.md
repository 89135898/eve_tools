# SDE Static Catalog Import Design

Date: 2026-05-26

## Purpose

Add a local EVE static catalog that can power item search, localization, item filtering, and market recommendation display without relying on one ESI metadata request per item.

This is a prerequisite for NPC Hub Selection Discovery. Discovery will receive large sets of `type_id` values from public market orders; the app needs local metadata to turn those IDs into useful, filterable recommendations.

## Source Data

Use CCP's official Static Data Export (SDE) from EVE Developers.

Primary source:

- `https://developers.eveonline.com/static-data/eve-online-static-data-latest-jsonl.zip`

Supporting metadata:

- `https://developers.eveonline.com/static-data/tranquility/latest.jsonl`
- `https://developers.eveonline.com/static-data/tranquility/schema-changelog.yaml`

The current checked source during design was build `3351823`, released `2026-05-19T12:12:31Z`. The implementation must not hard-code this build number; it should record whichever build was imported.

Use JSON Lines rather than YAML. The official static data documentation recommends JSON Lines for large files because it is more memory efficient and better suited to streaming.

## Scope

This design covers a static catalog MVP, not a full SDE mirror.

In scope:

- Download or read an SDE JSONL zip.
- Import item catalog data into SQLite.
- Store SDE build metadata.
- Store localized item, group, category, and market group names when present.
- Support fast local item lookup by type ID and name.
- Support filtering market-recommendation candidates by market eligibility and basic category metadata.

Out of scope:

- Dogma attributes and effects.
- Industry blueprints and materials.
- Universe geography beyond the fields needed by the trade hub config.
- Icons and image assets.
- Full station, structure, sovereignty, or faction metadata.
- Full in-app SDE update UI.
- Delta imports from SDE change files.

The schema should leave room for later full SDE support, but the first implementation should only import fields needed by market search and discovery.

## Data Files

The importer should read these SDE files from the JSONL zip:

- `_sde.jsonl`
- `types.jsonl`
- `groups.jsonl`
- `categories.jsonl`
- `marketGroups.jsonl`

Expected catalog fields:

The JSONL files use `_key` as the row's canonical ID. The importer should normalize `_key` into the app schema's `type_id`, `group_id`, `category_id`, and `market_group_id` fields. If a future SDE row also carries an explicit ID field such as `typeID`, the importer should validate that it matches `_key` and prefer `_key`.

### Types

- `_key` as `type_id`
- `name`
- `description`
- `groupID`
- `marketGroupID`
- `published`
- `volume`
- `packagedVolume`
- `capacity`
- `mass`
- `portionSize`
- `metaLevel` when present

### Groups

- `_key` as `group_id`
- `categoryID`
- `name`
- `published`

### Categories

- `_key` as `category_id`
- `name`
- `published`

### Market Groups

- `_key` as `market_group_id`
- `parentGroupID`
- `name`
- `description`

### SDE Metadata

- `buildNumber`
- `releaseDate`

The importer should tolerate missing optional fields and schema additions. Missing required IDs should cause the row to be skipped with an import warning, not a panic.

## Localization

SDE names and descriptions are localized maps. The catalog should preserve at least:

- English
- Simplified Chinese

Recommended language keys:

- `en`
- `zh`

If the SDE uses more specific keys such as `en-us` or `zh-cn`, normalize them into the app's supported language model at import time while preserving the raw value where useful.

Display fallback order:

1. Requested UI language.
2. English.
3. First available SDE name.
4. `Type <type_id>` fallback.

Search should support English and Chinese names. First implementation can use SQLite `LIKE` or FTS5 depending on what is simplest with the selected SQLite package. The design should not require a custom tokenizer in the first version.

## Dependency Policy

Do not hand-roll commodity infrastructure.

Allowed and preferred third-party crates:

- `zip` for reading the SDE zip archive.
- `serde` and `serde_json` for JSONL parsing.
- `reqwest` for HTTP download, reusing the existing workspace dependency.
- `rusqlite` or `sqlx` for SQLite access and transactions.
- `sha2` or a similar small crate if archive integrity checks are needed.
- `tempfile` for importer tests.

The implementation should prefer stable, widely used crates over custom zip parsing, custom JSON tokenization, or ad hoc SQL batching.

## Architecture

Add a static catalog import path under the Rust backend, keeping parsing, storage, and domain lookup separate.

Suggested module split:

```text
crates/
  sde/                 SDE archive reader, JSONL parsers, import service
  db/                  SQLite connection, migrations, catalog repositories
  domain/              catalog domain models and market filtering decisions
apps/
  desktop/src-tauri/   Tauri commands that call catalog services
```

If creating a new `crates/sde` crate feels too heavy during implementation, the same boundary can start inside `crates/db` or `crates/worker`, but parser logic should still be isolated from Tauri commands.

Recommended public boundaries:

```text
SdeArchiveSource
SdeCatalogImporter
CatalogRepository
ItemCatalogService
```

Tauri command handlers should not parse SDE files directly.

## Import Modes

Support two input modes:

1. Local archive import.
2. Official latest archive download.

Local archive import is important for tests and reproducible development. Official latest download is useful for normal users and developer setup.

Suggested commands or service functions:

- `import_sde_catalog_from_file(path)`
- `import_sde_catalog_latest()`
- `get_sde_catalog_status()`
- `search_inventory_types(query, language, limit)`
- `get_inventory_type(type_id, language)`

The first UI does not need a full update screen. A developer command or startup bootstrap is acceptable if documented, but the service should be designed so a settings screen can call it later.

## Storage Model

Use SQLite for the catalog. The database should be local to the desktop app profile once app storage is implemented.

Suggested tables:

### `sde_imports`

- `import_id`
- `build_number`
- `release_date`
- `source_url`
- `started_at`
- `completed_at`
- `status`
- `error_summary`
- `type_count`
- `group_count`
- `category_count`
- `market_group_count`

### `inventory_types`

- `type_id`
- `group_id`
- `market_group_id`
- `published`
- `volume`
- `packaged_volume`
- `capacity`
- `mass`
- `portion_size`
- `meta_level`
- `name_en`
- `name_zh`
- `description_en`
- `description_zh`
- `raw_name_json`
- `raw_description_json`
- `updated_import_id`

### `inventory_groups`

- `group_id`
- `category_id`
- `published`
- `name_en`
- `name_zh`
- `raw_name_json`
- `updated_import_id`

### `inventory_categories`

- `category_id`
- `published`
- `name_en`
- `name_zh`
- `raw_name_json`
- `updated_import_id`

### `market_groups`

- `market_group_id`
- `parent_group_id`
- `name_en`
- `name_zh`
- `description_en`
- `description_zh`
- `raw_name_json`
- `raw_description_json`
- `updated_import_id`

Indexes:

- `inventory_types(type_id)`
- `inventory_types(group_id)`
- `inventory_types(market_group_id)`
- `inventory_types(published)`
- `inventory_types(name_en)`
- `inventory_types(name_zh)`
- `inventory_groups(category_id)`
- `market_groups(parent_group_id)`

If FTS5 is used:

- `inventory_type_search(type_id, name_en, name_zh)`

The importer should write through a transaction. A failed import should not leave a half-imported active catalog.

## Market Eligibility

Discovery should only recommend items that can reasonably appear in market workflows.

Initial market eligibility rule:

- `published = true`
- `market_group_id IS NOT NULL`
- type has a non-empty display name
- optional exclusion list for categories or groups that are not useful for station trading

Do not overfit category exclusions in the static import phase. The recommendation engine can add scoring penalties later. The catalog should expose enough metadata for that engine to make decisions.

## Import Flow

Latest download flow:

1. Fetch `latest.jsonl`.
2. Parse build number and release date.
3. If that build is already successfully imported, return current status.
4. Download the latest JSONL zip.
5. Stream the needed files from the zip.
6. Parse rows with `serde_json`.
7. Normalize localized names.
8. Insert or replace rows inside a transaction.
9. Record counts and status in `sde_imports`.
10. Make the imported build active only after the transaction succeeds.

Local archive flow:

1. Open the user-provided or test archive path.
2. Read build metadata from `_sde.jsonl` if present; otherwise require explicit metadata from the caller or mark source as local unknown build.
3. Run the same parse, normalize, and write path.

## Error Handling

Expected error classes:

- Network failure while checking latest build.
- Network failure while downloading archive.
- Invalid or unsupported zip archive.
- Missing required SDE files.
- JSON parse failure for individual rows.
- SQLite migration or write failure.
- Disk-space failure.
- Import canceled or interrupted.

Behavior:

- Row-level parse failures should be counted and reported if the row is not critical.
- Missing entire required files should fail the import.
- A failed import should preserve the previous active catalog.
- The UI or command result should expose the last successful build and the latest failed import summary.

## Performance Direction

The importer should be streaming and transaction-based:

- Do not extract the full archive to permanent app storage unless needed.
- Do not load large JSONL files entirely into memory.
- Batch writes inside a single transaction or controlled chunk transactions.
- Build or refresh search indexes after rows are written.

The first implementation should prefer correctness and bounded memory over maximum import speed.

## Relationship To NPC Hub Discovery

NPC Hub Selection Discovery depends on this catalog for:

- mapping order `type_id` to localized item names
- filtering marketable published items
- grouping recommendations by category or market group
- showing useful display metadata
- avoiding one ESI type lookup per discovered item

Discovery should treat missing catalog rows as degraded data. It may show `Type <id>` for a row, but high-confidence recommendations should require catalog metadata once the catalog importer exists.

## Testing Strategy

Parser tests:

- parse representative `types.jsonl`, `groups.jsonl`, `categories.jsonl`, and `marketGroups.jsonl` rows
- handle missing optional fields
- normalize English and Chinese localized names
- tolerate extra fields from future SDE builds

Importer tests:

- import a small test zip into a temporary SQLite database
- verify counts and active build metadata
- verify failed imports preserve the previous active catalog
- verify required file missing causes a controlled error

Repository tests:

- lookup by type ID
- search by English name
- search by Chinese name
- filter market-eligible types
- retrieve group, category, and market group metadata

Desktop command tests:

- catalog status reports not imported before import
- local test archive import updates status
- search returns localized display names

Integration tests:

- static catalog can enrich a mocked market order type ID without ESI type lookup

## Documentation

README should document:

- why static SDE data is required
- how to import or refresh the catalog in development
- which official SDE source is used
- that the first importer is catalog-only, not full SDE coverage

## Migration Path

Phase 1:

- catalog import schema
- local test archive import
- search and type lookup services

Phase 2:

- official latest download
- startup or settings-triggered import
- frontend search integration

Phase 3:

- NPC Hub Discovery consumes catalog metadata

Phase 4:

- optional expanded SDE domains such as dogma, blueprints, icons, and universe geography
