# Catalog Standard Localization Design

Date: 2026-05-27

## Purpose

Normalize SDE localization data in Postgres so catalog APIs can return item, group, category, and market group names in the requested language without frontend-side SDE parsing or ad hoc language handling.

## Direction

- Keep static UI labels in React `react-i18next`.
- Move SDE entity localization selection to the Rust catalog service and repository.
- Store SDE names/descriptions in per-entity localization tables instead of relying on `name_en` and `name_zh` columns.
- Preserve existing columns for compatibility during the transition, but make new query behavior use localization tables first.
- Do not introduce a third-party i18n package for this step. The required fallback rule is small and deterministic.

## Tables

Add these tables:

- `evetools_catalog.inventory_type_localizations`
- `evetools_catalog.inventory_group_localizations`
- `evetools_catalog.inventory_category_localizations`
- `evetools_catalog.market_group_localizations`

Each table uses `(entity_id, language)` as the primary key and stores:

- `language`
- `name`
- `description` where the SDE entity has descriptions
- `updated_import_id`

The entity-specific table approach keeps foreign keys, indexes, and query plans simpler than a generic `(entity_kind, entity_id, language)` table.

## Import Behavior

The SDE parser exposes all non-empty localized names and descriptions as structured rows. The importer writes core entity rows first, then writes localization rows. A successful full import marks all localization rows with the current `updated_import_id` and deletes stale localization rows before deleting stale parent rows.

Repository same-build reuse must verify localization row counts as well as core row counts. This prevents an old full import without localization tables from blocking the first normalized localization import.

## Language Fallback

Repository queries receive a language string and resolve fallback candidates server-side:

1. exact language, after trimming and replacing `_` with `-`
2. base language before `-`, for example `zh` from `zh-Hans`
3. `zh` for any Chinese language tag
4. `en`
5. any available localization for the entity
6. generated fallback such as `Type 34`

Duplicate fallback candidates are removed while preserving order.

## Search

`search_inventory_types(query, language, limit)` searches localization names across languages so users can find items by any imported SDE name. Result display names still use the requested language fallback order.

## Compatibility

Existing Tauri command signatures remain unchanged:

- `search_inventory_types(query, language, limit)`
- `get_inventory_type(type_id, language)`

Existing TypeScript view shapes remain unchanged. The frontend still receives `display_name`, `group_name`, `category_name`, and `market_group_name`.
