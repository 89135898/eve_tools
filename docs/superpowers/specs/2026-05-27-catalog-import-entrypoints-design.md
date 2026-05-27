# Catalog Import Entrypoints Design

Date: 2026-05-27

## Purpose

Define how EveTools should run and maintain official SDE catalog imports after the first Supabase-backed catalog implementation.

The import logic must be complete and reusable in `CatalogService`. CLI, Tauri commands, and future hosted scheduled jobs are entrypoints only; they must not duplicate download, parse, skip, transaction, or repository logic.

## Confirmed Direction

- `CatalogService::import_latest()` remains the canonical official SDE import method.
- The method downloads official SDE metadata, compares it with the current catalog status, downloads the archive when needed, parses the JSONL zip, and imports it through `CatalogRepository`.
- Skipping an import requires evidence that the current row is a complete official import, not just a matching build number.
- A small admin CLI is useful for bootstrap, local development, and incident recovery.
- The CLI is not the long-term scheduler. A hosted job, worker, GitHub Actions workflow, or Supabase-hosted function can later call the same service logic.

## Skip Policy

`import_latest()` may skip downloading and importing only when all of these are true:

- latest catalog status is `success`
- latest catalog status build number equals the official latest build number
- latest catalog status `source_url` equals the official SDE archive URL
- imported row counts are above minimum completeness thresholds

The row-count guard prevents a one-row integration-test fixture from being treated as a complete official SDE import when it happens to use the same build number as the official latest build.

Initial thresholds are intentionally conservative and only distinguish full SDE from fixtures:

- types: at least 10,000
- groups: at least 500
- categories: at least 20
- market groups: at least 1,000

These thresholds should not be used as product metrics. They are only a defensive skip guard.

## Admin CLI

Add a binary under the existing `evetools-catalog` package:

```bash
EVETOOLS_DATABASE_URL="<supabase-postgres-url-with-sslmode-require>" \
  cargo run -p evetools-catalog --bin import-sde-latest
```

The CLI responsibilities are limited to:

- read `EVETOOLS_DATABASE_URL` through `CatalogConfig::from_env()`
- connect using `CatalogService::connect()`
- call `CatalogService::import_latest()`
- print status, build number, source URL, row counts, and completion time
- exit non-zero on failure without printing secrets

## Out Of Scope

- Scheduling.
- Retry/backoff orchestration.
- Progress UI.
- In-app import management UI.
- Production credential distribution to end-user desktop apps.

## Testing

- Unit-test the skip policy against a one-row fixture-like status with the official latest build number.
- Unit-test that complete official import status is skipped.
- Keep existing catalog repository integration tests.
- Compile the admin CLI through normal workspace tests.
