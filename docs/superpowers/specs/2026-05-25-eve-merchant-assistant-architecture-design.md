# EVE Merchant Assistant Architecture Design

Date: 2026-05-25

## Purpose

Build a personal EVE Online merchant assistant focused first on a Jita 4-4 station-trading workflow. The first architectural goal is to create a robust local desktop application that can support market price lookup, item selection discovery, and authenticated order monitoring before later expanding into broader trading, hauling, manufacturing, and portfolio analysis.

This document defines the architecture baseline. The current MVP product scope is defined in `docs/superpowers/specs/2026-05-25-jita-two-board-station-trading-mvp-design.md`.

## Current Scope Baseline

The first phase targets a Jita station trader who wants help finding tradeable items and monitoring active orders, not automated trading.

Initial assumptions:

- The app is a local single-user tool.
- The first market focus is Jita 4-4, using public The Forge/Jita market data where needed.
- The UI is a Tauri desktop app with a React/Vite interface.
- The backend owns market data sync, EVE SSO for private order monitoring, ESI access, local storage, synchronization, and analysis.
- The system provides recommendations and diagnostics only. It does not place, modify, or cancel EVE orders.
- Market price lookup is a shared capability used by both selection discovery and order monitoring.

## Recommended Stack

Use a Tauri desktop app with a Rust core and a TypeScript UI:

- Frontend: React, Vite, TanStack Query, TanStack Table
- Desktop shell: Tauri 2
- Backend/core: Rust, Tokio
- Storage: SQLite for MVP, with schema discipline that allows later PostgreSQL migration
- Database access: SQLx
- HTTP client: reqwest
- Serialization and validation: serde plus explicit domain validation
- Money math: rust_decimal or integer minor-unit modeling where appropriate
- Scheduling: simple Tokio-based in-process scheduler for MVP, with a clear path to a durable queue if job reliability requires it
- Deployment shape: local desktop development first; a Web/server adapter can be added later if remote access becomes useful

The primary reason for this stack is a stable, strongly typed Rust core for ESI sync, token handling, snapshots, and trading calculations while keeping the UI fast to build in React. TypeScript remains the UI language only; backend business logic should live in Rust. Tauri command handlers should be thin adapters over `crates/domain`, `crates/esi`, `crates/db`, and `crates/worker`.

## Architecture

The system should be structured as a backend-driven analysis tool:

```text
Tauri Desktop App
        |
React/Vite UI
        |
Tauri Commands
        |
Application Services
        |
Domain Engine
        |
Database
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
  worker/           Sync orchestration and scheduled jobs
apps/
  desktop/          Tauri shell, React UI, command handlers, SSO callback adapter
```

The most important rule is that React must not own business calculations. The frontend requests prepared analysis results and renders them. Price lookup, spread calculation, liquidity scoring, selection ranking, order urgency, stale-data detection, risk tags, and repricing suggestions live in `crates/domain`.

## Data Flow

1. The user opens the desktop app.
2. The backend resolves item metadata and public Jita market data for price lookup and selection discovery.
3. Public ESI responses are validated and stored as snapshots.
4. If order monitoring is used, the backend starts EVE SSO and stores refresh credentials locally after authorization.
5. Authenticated sync jobs pull character orders and relevant Jita market data from ESI.
6. Domain services compute price state, item opportunity, order urgency, risk, and suggested actions.
7. The UI invokes Tauri commands to read analyzed views from the Rust core.

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

- Store authorized character metadata and encrypted or locally protected refresh credentials where practical.
- Store candidate pools, watchlists, fee profiles, and app settings.
- Store personal order snapshots.
- Store market order snapshots for relevant Jita items.
- Store sync attempts, cache metadata, and failure reasons.

## Data Model Direction

Exact tables will be refined later, but the schema should include these concepts:

- `characters`
- `auth_tokens`
- `candidate_items`
- `watchlist_items`
- `fee_profiles`
- `price_lookup_history`
- `character_order_snapshots`
- `market_order_snapshots`
- `market_history_snapshots`
- `item_types`
- `stations`
- `sync_runs`
- `analysis_snapshots`

The schema should avoid storing only the latest state. Historical snapshots are needed for future analysis such as order aging, fill speed, price movement, and stale inventory detection.

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
- Whether to support multiple characters in the first release.
- Whether sync should be manual-only or scheduled in MVP.
- Whether to add a separate Web/server adapter later.
- Whether to migrate from SQLite to PostgreSQL.
- Whether to introduce a durable queue for background jobs.
- Whether to rewrite selected modules in Rust after the workflow is proven.

## First Implementation Direction

When implementation begins, start with the smallest vertical slice:

1. Create the monorepo structure.
2. Add `crates/domain` with tested price-state, spread, liquidity, and repricing primitives.
3. Add stubbed Tauri commands that return fixture-based market lookup, selection, and order-monitor views.
4. Add the desktop UI shell against those commands.
5. Add public ESI market sync for price lookup and selection discovery.
6. Add EVE SSO and authenticated character-order sync after the public analysis workflow is visible.

This sequence lets the project validate the merchant workflow before spending too much time on integration details.
