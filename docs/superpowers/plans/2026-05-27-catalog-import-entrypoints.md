# Catalog Import Entrypoints Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make official SDE import reusable through `CatalogService` while adding a thin admin CLI for bootstrap and recovery.

**Architecture:** `CatalogService::import_latest()` owns official import behavior and skip decisions. The CLI is a binary entrypoint in the `evetools-catalog` package and only calls the service. Future scheduled jobs can reuse the same service method.

**Tech Stack:** Rust 1.82, `tokio`, `sqlx`, Supabase Postgres via `EVETOOLS_DATABASE_URL`, existing `evetools-catalog`, `evetools-db`, and `evetools-sde` crates.

---

## Task 1: Guard Official Import Skip Logic

**Files:**

- Modify: `crates/catalog/src/lib.rs`

- [ ] **Step 1: Write failing tests**

Add tests proving that a fixture-sized status with the official latest build must not skip, while a complete official status can skip.

- [ ] **Step 2: Run test to verify failure**

Run:

```bash
cargo test -p evetools-catalog import_skip
```

Expected: fail because the helper does not exist.

- [ ] **Step 3: Implement helper and wire it into `import_latest()`**

Add a private helper that checks status, build number, source URL, and row-count thresholds before skipping.

- [ ] **Step 4: Run catalog tests**

Run:

```bash
cargo test -p evetools-catalog
```

Expected: pass.

## Task 2: Guard Repository Import Reuse

**Files:**

- Modify: `crates/db/src/catalog.rs`

- [ ] **Step 1: Add repository idempotency tests**

Add tests proving same-build imports are reused only when source URL and row counts match.

- [ ] **Step 2: Implement repository guard**

Update `CatalogRepository::import_archive()` so `latest_success_status_for_build()` returns early only when the previous successful import matches the incoming source URL and archive counts.

- [ ] **Step 3: Run DB tests**

Run:

```bash
cargo test -p evetools-db import_reuse
cargo test -p evetools-db --test catalog_repository
```

Expected: pass. The integration test may skip if `EVETOOLS_TEST_DATABASE_URL` is not set.

## Task 3: Add Thin Admin CLI

**Files:**

- Modify: `crates/catalog/Cargo.toml`
- Create: `crates/catalog/src/bin/import-sde-latest.rs`

- [ ] **Step 1: Add `tokio` dependency to `evetools-catalog`**

The binary needs `#[tokio::main]`.

- [ ] **Step 2: Create binary**

The binary reads `EVETOOLS_DATABASE_URL`, connects with `CatalogService`, calls `import_latest()`, prints non-secret status data, and exits with code 1 on error.

- [ ] **Step 3: Compile through tests**

Run:

```bash
cargo test -p evetools-catalog
```

Expected: pass and compile the binary.

## Task 4: Document Admin Import Command

**Files:**

- Modify: `README.md`

- [ ] **Step 1: Add bootstrap command**

Document the `cargo run -p evetools-catalog --bin import-sde-latest` command and clarify that `EVETOOLS_TEST_DATABASE_URL` must not point to the same database used for full catalog imports.

- [ ] **Step 2: Verify docs and workspace**

Run:

```bash
cargo fmt --all -- --check
cargo test --workspace
pnpm --filter @evetools/desktop typecheck
git diff --check
```

Expected: all pass.

## Task 5: Add CLI Progress Output

**Files:**

- Modify: `crates/db/src/catalog.rs`
- Modify: `crates/db/src/lib.rs`
- Modify: `crates/catalog/src/lib.rs`
- Modify: `crates/catalog/src/bin/import-sde-latest.rs`
- Modify: `README.md`

- [ ] **Step 1: Add failing progress tests**

Add one DB unit test for row progress throttling and one CLI unit test for formatting row progress and downloaded archive size.

- [ ] **Step 2: Implement progress events**

Add table progress events in `evetools-db`, expose catalog service progress events in `evetools-catalog`, and keep existing no-progress methods as wrappers.

- [ ] **Step 3: Print progress in CLI**

Update `import-sde-latest` to call `CatalogService::import_latest_with_progress()` and format each emitted event.

- [ ] **Step 4: Verify workspace**

Run:

```bash
cargo fmt --all -- --check
cargo test --workspace
pnpm --filter @evetools/desktop typecheck
git diff --check
```

Expected: all pass.
