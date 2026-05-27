# Catalog Standard Localization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Normalize SDE localization data into per-entity Postgres tables and make catalog APIs choose display names server-side using language fallback.

**Architecture:** `crates/sde` parses all localized names/descriptions into structured localization rows. `crates/db` owns localization tables, import writes, reuse guards, and language-aware queries. `crates/catalog` and Tauri APIs keep their existing public shape and pass language strings through to the repository.

**Tech Stack:** Rust 1.82, `serde_json`, `sqlx` Postgres, Supabase Postgres, existing Tauri command wrappers.

---

## Task 1: Parse SDE Localizations

**Files:**

- Modify: `crates/sde/src/models.rs`
- Modify: `crates/sde/src/parser.rs`
- Modify: `crates/sde/tests/parser.rs`

- [ ] **Step 1: Add parser tests for extra languages**

Add tests proving a type row exposes non-English/non-Chinese localized names and descriptions as structured localization entries.

- [ ] **Step 2: Add localization model**

Add `CatalogLocalization { language, name, description }` and localization vectors to catalog entities.

- [ ] **Step 3: Run parser tests**

Run:

```bash
cargo test -p evetools-sde --test parser
```

Expected: pass.

## Task 2: Add Localization Schema And Import Writes

**Files:**

- Modify: `crates/db/src/schema.rs`
- Modify: `crates/db/src/catalog.rs`
- Modify: `crates/db/tests/catalog_repository.rs`

- [ ] **Step 1: Add schema tests**

Add tests proving localization tables and indexes are present.

- [ ] **Step 2: Create localization tables**

Add per-entity localization tables with `(entity_id, language)` primary keys and `updated_import_id`.

- [ ] **Step 3: Write localization rows during import**

Insert localizations after parent rows and before stale-row deletion. Delete stale localization rows before parent rows.

- [ ] **Step 4: Guard same-build reuse with localization counts**

Existing same-build imports may be reused only when core counts and localization counts match the incoming archive.

## Task 3: Query With Server-Side Language Fallback

**Files:**

- Modify: `crates/db/src/catalog.rs`

- [ ] **Step 1: Add fallback unit tests**

Test `zh-Hans -> zh -> en`, `en-US -> en`, duplicate removal, and empty language fallback.

- [ ] **Step 2: Update lookup query**

Use localization tables to compute display names for type, group, category, and market group.

- [ ] **Step 3: Update search query**

Search `inventory_type_localizations.name` across languages and keep display output in the requested language.

## Task 4: Docs And Verification

**Files:**

- Modify: `README.md`

- [ ] **Step 1: Document localization behavior**

Explain that SDE entity names are selected server-side and that a reimport is required after this schema change.

- [ ] **Step 2: Verify workspace**

Run:

```bash
cargo fmt --all -- --check
cargo test --workspace
pnpm --filter @evetools/desktop typecheck
git diff --check
```

Expected: all pass.
