# SDE Static Catalog Import Design

Date: 2026-05-26

## Purpose

Add an EVE static catalog service that imports CCP's official Static Data Export (SDE) into Supabase Postgres and exposes item search, localization, market eligibility, and type lookup through Rust service APIs.

This is a prerequisite for NPC Hub Selection Discovery. Discovery receives large sets of `type_id` values from public market orders; the app needs server-side catalog metadata to turn those IDs into useful recommendations.

## Confirmed Direction

- First version uses Supabase Postgres as the catalog database.
- SDE parsing, importing, and querying live in Rust service code.
- React desktop code never parses SDE files and never connects directly to the database.
- Tauri commands call Rust catalog service functions and receive prepared view models.
- Database URLs and credentials are never committed. They must be read from environment variables.

The database credentials previously pasted into chat should be treated as exposed and rotated in Supabase before implementation uses them.

## Source Data

Use CCP's official SDE JSON Lines archive:

- `https://developers.eveonline.com/static-data/eve-online-static-data-latest-jsonl.zip`

Supporting metadata:

- `https://developers.eveonline.com/static-data/tranquility/latest.jsonl`
- `https://developers.eveonline.com/static-data/tranquility/schema-changelog.yaml`

The current checked source during design was build `3351823`, released `2026-05-19T12:12:31Z`. The implementation must not hard-code this build number.

Use JSON Lines rather than YAML because it is better suited to streaming and large file parsing.

## Supabase Connection Policy

Use an environment variable:

- `EVETOOLS_DATABASE_URL`

The value should be a Supabase Postgres URL with SSL enabled, for example with `sslmode=require`. Do not store actual credentials in source files, plans, README examples, tests, or screenshots.

For the first importer, prefer the direct Postgres connection for migrations and large imports. Supabase's pooler is useful for many short-lived client sessions, but the importer performs long transactions and bulk writes. The pooler can be evaluated later for read-only query workloads.

The desktop app must not ship a privileged database URL to end users. This Supabase-first implementation is acceptable for a private/local tool and development. Before distributing the app, replace direct DB access with a hosted API, Supabase Edge Function, or strict RLS-backed public read path.

## Scope

In scope:

- Download or read an SDE JSONL zip in Rust.
- Parse `_sde.jsonl`, `types.jsonl`, `groups.jsonl`, `categories.jsonl`, and `marketGroups.jsonl`.
- Normalize `_key` into internal IDs.
- Import catalog rows into Supabase Postgres.
- Store import metadata and row counts.
- Preserve English and Chinese names/descriptions plus raw localized JSON.
- Expose Rust service methods for status, import latest, import local archive, search, lookup, and market eligibility.
- Expose Tauri commands that call the Rust catalog service.

Out of scope:

- Dogma attributes and effects.
- Industry blueprints and materials.
- Icons and image assets.
- Full universe geography.
- Full in-app import/update UI.
- Delta imports from SDE change files.
- Public production credential distribution.

## Data Files And Fields

The importer reads:

- `_sde.jsonl`
- `types.jsonl`
- `groups.jsonl`
- `categories.jsonl`
- `marketGroups.jsonl`

The JSONL files use `_key` as the row's canonical ID. The importer normalizes `_key` into `type_id`, `group_id`, `category_id`, and `market_group_id`.

Type fields:

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

Group fields:

- `_key` as `group_id`
- `categoryID`
- `name`
- `published`

Category fields:

- `_key` as `category_id`
- `name`
- `published`

Market group fields:

- `_key` as `market_group_id`
- `parentGroupID`
- `name`
- `description`

SDE metadata fields:

- `buildNumber`
- `releaseDate`

Missing optional fields are allowed. Missing required IDs cause that row to be counted as rejected with an import warning.

## Localization

Preserve at least:

- English: `en`
- Simplified Chinese: `zh`

Display fallback order:

1. Requested UI language.
2. English.
3. First available SDE name.
4. `Type <type_id>`.

Search supports English and Chinese names. The first version can use indexed `ILIKE` queries. PostgreSQL full-text or trigram search can be added later.

## Architecture

Use a Rust service boundary:

```text
React UI
  |
Tauri commands
  |
crates/catalog        application service, config, orchestration
  |
crates/sde            SDE download, zip reading, JSONL parsing
  |
crates/db             Supabase/Postgres schema and repository
  |
Supabase Postgres
```

Responsibilities:

- `crates/sde`: pure SDE source handling. It knows about zip files, JSONL rows, and official download URLs.
- `crates/db`: Postgres schema migration and catalog repository queries.
- `crates/catalog`: application service that coordinates SDE import/download and repository operations.
- `apps/desktop/src-tauri`: command adapter only. It reads config, calls `CatalogService`, and returns typed views.
- `apps/desktop/src`: typed command wrappers only. No database access.

## Database Model

Use a dedicated schema:

- `evetools_catalog`

Tables:

- `evetools_catalog.sde_imports`
- `evetools_catalog.inventory_types`
- `evetools_catalog.inventory_groups`
- `evetools_catalog.inventory_categories`
- `evetools_catalog.market_groups`

Important indexes:

- `inventory_types(type_id)`
- `inventory_types(group_id)`
- `inventory_types(market_group_id)`
- `inventory_types(published)`
- `inventory_types(name_en)`
- `inventory_types(name_zh)`
- `inventory_groups(category_id)`
- `market_groups(parent_group_id)`

The import should run inside a transaction. A failed import must preserve the previous successful catalog.

## Public Service API

Rust service methods:

- `CatalogService::status()`
- `CatalogService::import_archive(path)`
- `CatalogService::import_latest()`
- `CatalogService::search_inventory_types(query, language, limit)`
- `CatalogService::get_inventory_type(type_id, language)`
- `CatalogService::market_eligible_types(limit)`

Tauri command names:

- `get_sde_catalog_status`
- `import_sde_catalog_from_file`
- `import_sde_catalog_latest`
- `search_inventory_types`
- `get_inventory_type`

View models:

- `CatalogStatus`
- `InventoryTypeView`

React receives these view models through Tauri only.

## Market Eligibility

Initial rule:

- `published = true`
- `market_group_id IS NOT NULL`
- at least one display name exists

The catalog service exposes eligibility as data. The recommendation engine can add scoring penalties or exclusions later.

## Error Handling

Expected errors:

- Missing `EVETOOLS_DATABASE_URL`.
- Supabase connection failure.
- Migration failure.
- SDE latest metadata download failure.
- SDE archive download failure.
- Invalid zip archive.
- Missing required SDE file.
- JSONL row parse failure.
- Transaction rollback after import failure.

Behavior:

- Missing config returns a clear service error.
- Failed import does not replace the last successful import.
- Row-level parse failures are counted and surfaced.
- Missing required files fail the import.
- Tauri commands convert service errors into user-visible strings without leaking credentials.

## Testing

Parser tests:

- parse representative SDE rows
- normalize `_key`
- preserve English and Chinese names
- tolerate missing optional fields

Archive tests:

- read a small generated JSONL zip
- fail cleanly when required files are missing

Repository tests:

- run migrations against Postgres when `EVETOOLS_TEST_DATABASE_URL` is set
- import a small catalog archive transactionally
- query by type ID
- search English and Chinese names
- filter market-eligible types
- preserve previous rows after failed import

Service tests:

- missing database URL returns config error
- service delegates status/search/lookup to repository
- import local archive coordinates parser and repository

Desktop command tests:

- missing database config returns a controlled error
- command wrappers compile and return typed shapes in unit tests where possible

## Documentation

README should document:

- official SDE source URL
- required `EVETOOLS_DATABASE_URL`
- recommendation to rotate exposed credentials
- Supabase direct connection recommendation for imports
- no secrets in source control
- React uses Tauri commands rather than direct DB access
