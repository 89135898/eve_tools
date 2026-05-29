# EVE Merchant Assistant Architecture Design

Date: 2026-05-25

## Purpose

Build a personal EVE Online merchant assistant focused first on a Jita 4-4 station-trading workflow. The first architectural goal is to create a robust local desktop application that can support market price lookup, item selection discovery, and authenticated order monitoring before later expanding into broader trading, hauling, manufacturing, and portfolio analysis.

This document defines the architecture baseline. The current MVP product scope is defined in `docs/superpowers/specs/2026-05-25-jita-two-board-station-trading-mvp-design.md`.

Status update, 2026-05-29: the implemented architecture has moved past the original local-only storage sketch. Public market and catalog reads now use a hosted HTTP API backed by Supabase/Postgres snapshots. The desktop app still owns the Tauri shell and local/private EVE SSO flow, but React does not receive tokens or database credentials.

## Current Scope Baseline

The first phase targets a Jita station trader who wants help finding tradeable items and monitoring active orders, not automated trading.

Initial assumptions:

- The app is desktop-first and can use hosted read APIs for catalog and public market data.
- The first market focus is Jita 4-4, with Selection Discovery expanded to the major NPC hubs: Jita, Amarr, Dodixie, Rens, and Hek.
- The UI is a Tauri desktop app with a React/Vite interface.
- Rust services own market data sync, EVE SSO for private order monitoring, ESI access, Postgres persistence, synchronization, and analysis.
- The system provides recommendations and diagnostics only. It does not place, modify, or cancel EVE orders.
- Market price lookup is a shared capability used by both selection discovery and order monitoring.

## Recommended Stack

Use a Tauri desktop app with a Rust core and a TypeScript UI:

- Frontend: React, Vite, TanStack Query, TanStack Table
- Desktop shell: Tauri 2
- Backend/core: Rust, Tokio
- Storage: Supabase/Postgres with SQLx migrations and repositories
- Database access: SQLx
- HTTP client: reqwest
- Hosted read API: Axum
- Serialization and validation: serde plus explicit domain validation
- Money math: rust_decimal or integer minor-unit modeling where appropriate
- Scheduling: external scheduler or manual worker CLI invoking Rust worker entrypoints
- Deployment shape: desktop client plus hosted HTTP API/worker/admin processes for production data paths

The primary reason for this stack is a stable, strongly typed Rust core for ESI sync, token handling, snapshots, and trading calculations while keeping the UI fast to build in React. TypeScript remains the UI language only; backend business logic should live in Rust. Tauri command handlers should be thin adapters over `crates/domain`, `crates/esi`, `crates/db`, and `crates/worker`.

## Architecture

The system should be structured as a backend-driven analysis tool:

```text
Tauri Desktop App
        |
React/Vite UI
        |
Tauri Commands + hosted HTTP API
        |
Application Services / Read API
        |
Domain Engine + Worker
        |
Supabase/Postgres
        |
Background Sync
        |
Public ESI / EVE SSO / Authenticated ESI
```

Suggested project layout:

```text
crates/
  domain/           Spread, liquidity, profitability, repricing, urgency, ranking
  esi/              Public and authenticated ESI clients, response validation, cache metadata
  db/               SQLx schema access, migrations, repositories
  api/              Read-side application API over catalog and market repositories
  http-api/         Axum hosted adapter for read routes and health
  catalog/          SDE catalog import/query service
  sde/              SDE archive discovery and parsing
  worker/           Sync orchestration and scheduled jobs
apps/
  desktop/          Tauri shell, React UI, command handlers, SSO callback adapter
```

The most important rule is that React must not own business calculations. The frontend requests prepared analysis results and renders them. Price lookup, spread calculation, liquidity scoring, selection ranking, order urgency, stale-data detection, risk tags, and repricing suggestions live in `crates/domain`.

## Data Flow

1. The user opens the desktop app.
2. Public market workers validate ESI responses and store latest successful snapshots in Supabase/Postgres.
3. The hosted HTTP API resolves catalog metadata, market lookup, trade hubs, selection candidates, and sync health from those snapshots.
4. The desktop app calls hosted read routes through `EVETOOLS_API_BASE_URL` for public data.
5. If order monitoring is used in local/private desktop mode, Tauri Rust starts EVE SSO with PKCE and stores refresh credentials in Rust-owned Postgres tables after authorization.
6. Authenticated sync jobs pull character orders from ESI and persist character order snapshots.
7. Domain services compute price state, item opportunity, order urgency, risk, and suggested actions.
8. The UI invokes Tauri commands and renders prepared view models; React does not handle ESI tokens.

The app should prefer snapshot-based analysis over purely live API reads. Snapshots make it possible to reason about price drift, liquidity changes, order age, filled quantity, stale items, and trend changes.

## Module Boundaries

### Desktop UI

Responsibilities:

- Display market price lookup results.
- Display selection discovery results.
- Display order tables, filters, detail panels, and charts.
- Trigger manual sync.
- Show sync status and stale-data warnings.
- Present backend-generated recommendations.

Non-responsibilities:

- ESI token handling.
- Direct ESI calls.
- Profit and fee calculations.
- Liquidity and selection scoring.
- Repricing logic.

### Tauri Command Adapter

Responsibilities:

- Expose safe desktop commands for market lookup, sync, settings, SSO, and monitor views.
- Resolve item lookup and market price lookup requests through application services.
- Handle EVE SSO login and callback.
- Expose analyzed view models to the React UI.
- Trigger and report sync jobs.
- Enforce validation at request and response boundaries.

### ESI Client

Responsibilities:

- Call public and authenticated EVE ESI endpoints.
- Handle pagination, cache headers, retries, and ESI error responses.
- Validate response shape.
- Return normalized data to application services.

### Domain Engine

Responsibilities:

- Summarize Jita price state for an item.
- Score item liquidity and selection quality.
- Calculate net profit and margin.
- Compare personal orders with market orders.
- Classify orders by urgency and risk.
- Produce repricing and attention recommendations.

Domain code must be deterministic and testable without network or database access.

### Storage

Responsibilities:

- Store authorized character metadata and refresh credentials in Rust-owned tables; do not expose them to React/WebView state.
- Store catalog metadata, localizations, trade hubs, public market sync runs, market order snapshots, character order sync runs, and character order snapshots.
- Store future candidate pools, watchlists, fee profiles, and app settings when those product surfaces are added.
- Store personal order snapshots.
- Store market order snapshots for configured NPC trade hubs.
- Store sync attempts, cache metadata, and failure reasons.

## Data Model Direction

The current schema is Postgres-first and includes these implemented concepts:

- catalog entity and localization tables
- `trade_hubs`
- `market_sync_runs`
- `market_order_snapshots`
- `characters`
- `character_auth_tokens`
- `character_order_sync_runs`
- `character_order_snapshots`

Future product tables can add watchlists, fee profiles, wallet/transaction views, market history snapshots, and analysis snapshots. The schema should avoid storing only the latest state. Historical snapshots are needed for future analysis such as order aging, fill speed, price movement, and stale inventory detection.

## Reliability Rules

- Never store EVE refresh tokens in WebView storage or frontend state.
- Respect ESI cache and error-limit behavior.
- Treat every ESI response as untrusted input and validate it.
- Make sync idempotent where possible.
- Persist sync failure reasons instead of only logging them.
- Show data freshness in the UI.
- Keep decimal calculations out of JavaScript floating-point arithmetic where precision matters.

## Testing Strategy

The first implementation plan should include:

- Unit tests for domain calculations.
- Fixture-based tests for ESI response normalization.
- Command/API adapter tests for auth state, sync status, and analyzed order views.
- Frontend smoke tests for core dashboard states once UI exists.

Profit, fee, and repricing formulas need focused test coverage because small calculation errors can produce bad trading advice.

## Deferred Decisions

These items are intentionally left for later product design:

- Exact dashboard layout.
- Exact first set of order recommendations.
- Fee model defaults and user overrides.
- How to estimate acquisition cost for sell orders.
- Whether to support full multi-character switching in the first release.
- Whether authenticated character order sync should remain manual-only or become scheduled.
- Whether to add a separate Web/server adapter later.
- Whether to move local/private desktop SSO token storage into a controlled hosted backend before distributing to untrusted users.
- Whether to introduce a durable queue for background jobs.

## Current Implementation Status

The initial vertical slices are now implemented as:

1. Rust workspace crates for domain, ESI, DB, catalog, read API, HTTP API, worker, and desktop adapter.
2. Tested domain calculations for market lookup, selection scoring, and advisory repricing.
3. Supabase/Postgres catalog and market snapshot persistence via SQLx migrations.
4. Public ESI market sync worker for configured NPC trade hubs.
5. Hosted HTTP read API for catalog, market lookup, selection candidates, station orders, sync health, and authenticated order monitor rows.
6. Tauri desktop UI wired to hosted reads plus Rust-owned EVE SSO, token storage, character order sync, and Order Monitor views.
7. Explicit fixture mode for local development and deterministic tests only.

Next architecture work should focus on hardening production secret boundaries, hosted authenticated sync for multi-user distribution, richer portfolio data, and operational alerting.
