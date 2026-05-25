# EVE Merchant Assistant Architecture Design

Date: 2026-05-25

## Purpose

Build a personal EVE Online merchant assistant focused first on a Jita single-station order trader workflow. The first architectural goal is to create a robust local Web Dashboard foundation that can later support broader order trading, hauling, manufacturing, and portfolio analysis.

This document intentionally defines the architecture baseline before detailed product requirements are finalized. Feature scope, ranking formulas, and exact dashboard views will be refined in later design discussions.

## Current Scope Baseline

The first phase targets a personal order merchant who wants operational help managing existing orders, not automated trading.

Initial assumptions:

- The app is a local single-user tool.
- The first market focus is Jita 4-4.
- The UI is a browser-based dashboard.
- The backend owns EVE SSO, ESI access, local storage, synchronization, and analysis.
- The system provides recommendations and diagnostics only. It does not place, modify, or cancel EVE orders.

## Recommended Stack

Use a TypeScript Web Dashboard architecture:

- Frontend: React, Vite, TanStack Query, TanStack Table
- Backend: Node.js LTS, TypeScript, Fastify
- Storage: SQLite for MVP, with schema discipline that allows later PostgreSQL migration
- Validation: Zod or Fastify JSON Schema at external boundaries
- Money math: decimal.js or an equivalent decimal arithmetic library
- Scheduling: simple in-process scheduler for MVP, with a clear path to BullMQ plus Redis if durable jobs become necessary
- Deployment shape: local development first, Docker Compose later if server deployment becomes useful

The primary reason for this stack is iteration speed with clear module boundaries. Rust remains a viable future option for replacing specific backend components, but the first version should prioritize getting the order workflow and analysis model right.

## Architecture

The system should be structured as a backend-driven analysis tool:

```text
React Web Dashboard
        |
Fastify API
        |
Application Services
        |
Domain Engine
        |
Database
        |
Background Sync
        |
EVE SSO / ESI
```

Suggested project layout:

```text
apps/
  api/              Fastify API, auth callbacks, sync orchestration
  web/              React dashboard
packages/
  domain/           Profit, repricing, risk, ranking, order classification
  esi/              ESI client, response validation, cache metadata
  db/               Schema, migrations, repositories
  shared/           Shared types that are safe to expose to UI
```

The most important rule is that React must not own business calculations. The frontend requests prepared analysis results and renders them. Profit calculation, order urgency, stale-order detection, risk tags, and repricing suggestions live in `packages/domain`.

## Data Flow

1. The user opens the local dashboard.
2. If no character is authorized, the backend starts EVE SSO.
3. The backend receives the OAuth callback and stores refresh credentials locally.
4. A sync job pulls character orders and relevant Jita market data from ESI.
5. ESI responses are validated and stored as snapshots.
6. Domain services compute order status, opportunity, risk, and suggested actions.
7. The dashboard reads analyzed API views from the backend.

The app should prefer snapshot-based analysis over purely live API reads. Snapshots make it possible to reason about order age, price drift, filled quantity, stale items, and trend changes.

## Module Boundaries

### Web Dashboard

Responsibilities:

- Display order tables, filters, detail panels, and charts.
- Trigger manual sync.
- Show sync status and stale-data warnings.
- Present backend-generated recommendations.

Non-responsibilities:

- ESI token handling.
- Direct ESI calls.
- Profit and fee calculations.
- Repricing logic.

### API

Responsibilities:

- Host local HTTP endpoints for the dashboard.
- Handle EVE SSO login and callback.
- Expose analyzed dashboard views.
- Trigger and report sync jobs.
- Enforce validation at request and response boundaries.

### ESI Client

Responsibilities:

- Call EVE ESI endpoints.
- Handle pagination, cache headers, retries, and ESI error responses.
- Validate response shape.
- Return normalized data to application services.

### Domain Engine

Responsibilities:

- Calculate net profit and margin.
- Compare personal orders with market orders.
- Classify orders by urgency and risk.
- Produce repricing and attention recommendations.

Domain code must be deterministic and testable without network or database access.

### Storage

Responsibilities:

- Store authorized character metadata and encrypted or locally protected refresh credentials where practical.
- Store personal order snapshots.
- Store market order snapshots for relevant Jita items.
- Store sync attempts, cache metadata, and failure reasons.

## Data Model Direction

Exact tables will be refined later, but the schema should include these concepts:

- `characters`
- `auth_tokens`
- `personal_order_snapshots`
- `market_order_snapshots`
- `market_history_snapshots`
- `item_types`
- `stations`
- `sync_runs`
- `analysis_snapshots`

The schema should avoid storing only the latest state. Historical snapshots are needed for future analysis such as order aging, fill speed, price movement, and stale inventory detection.

## Reliability Rules

- Never store EVE refresh tokens in browser local storage.
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
- API tests for auth state, sync status, and analyzed order views.
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
- Whether to package the local Web Dashboard into a desktop app later.
- Whether to migrate from SQLite to PostgreSQL.
- Whether to introduce Redis and BullMQ for durable background jobs.
- Whether to rewrite selected modules in Rust after the workflow is proven.

## First Implementation Direction

When implementation begins, start with the smallest vertical slice:

1. Create the monorepo structure.
2. Add the domain package with tested money and order-analysis primitives.
3. Add a stubbed API that returns fixture-based analyzed orders.
4. Add the dashboard table against that API.
5. Add EVE SSO and real ESI sync only after the local analysis workflow is visible.

This sequence lets the project validate the merchant workflow before spending too much time on integration details.
