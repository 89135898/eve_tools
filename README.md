# EveTools

EveTools is a desktop-first EVE Online station trading assistant focused on Jita 4-4.

The current implementation slice provides:

- Tauri 2 desktop shell
- React/Vite desktop UI
- Rust workspace crates for domain logic, ESI, Supabase catalog data, and workers
- Tested Rust domain calculations for spread, fees, liquidity, and attention scoring
- Public ESI-backed market price lookup with fixture fallback
- Public ESI-backed selection discovery with fixture fallback
- Fixture-backed order monitor

This slice uses live public ESI for market lookup and selection discovery when available, while keeping deterministic fixture fallback for development and outages. Static SDE catalog data is imported into Supabase Postgres through the Rust catalog service. EVE SSO and authenticated character-order sync are deferred to later implementation phases.

## Development

Install dependencies:

```sh
pnpm install
```

Run all checks:

```sh
pnpm check
```

Run Rust tests only:

```sh
pnpm test:rust
```

Run TypeScript type checking only:

```sh
pnpm typecheck
```

Start the desktop app:

```sh
pnpm dev
```

Build the desktop app:

```sh
pnpm build
```

## Public ESI Market Sync

The desktop app can use live public ESI data for the Jita market lookup and selection board.

Market source mode is controlled by `EVETOOLS_MARKET_SOURCE`:

```bash
EVETOOLS_MARKET_SOURCE=live pnpm dev
EVETOOLS_MARKET_SOURCE=fixture pnpm dev
```

When the variable is omitted, the backend uses `live`.

Public ESI mode currently uses these unauthenticated endpoints:

- `POST /universe/ids/`
- `GET /universe/types/{type_id}/`
- `GET /markets/{region_id}/orders/`
- `GET /markets/{region_id}/history/`

The current public slice is intentionally small:

- The Forge region only.
- Jita 4-4 station orders only for top-of-book analysis.
- A fixed seed pool for selection discovery.
- Fixture fallback on public ESI network, status, or decode failure.

Authenticated character order monitoring remains fixture-backed until the SSO phase.

## Static SDE Catalog

EveTools imports CCP's official SDE JSON Lines archive into Supabase Postgres through the Rust catalog service.

Required environment variable:

```bash
export EVETOOLS_DATABASE_URL="<supabase-postgres-url-with-sslmode-require>"
```

Do not commit real database URLs or passwords. If a credential is pasted into chat, logs, or source control, rotate it in Supabase before use.

The first catalog slice imports:

- `_sde.jsonl`
- `types.jsonl`
- `groups.jsonl`
- `categories.jsonl`
- `marketGroups.jsonl`

React does not connect to Supabase directly. It calls Tauri commands, and Tauri calls the Rust catalog service.

## Architecture

Business logic lives in Rust crates:

- `crates/domain`: market models, price calculations, scoring, serialized view models, and fixtures.
- `crates/esi`: ESI client boundary shell.
- `crates/sde`: SDE JSON Lines archive discovery and record parsing.
- `crates/db`: Supabase/Postgres catalog schema and repository.
- `crates/catalog`: Rust catalog service for importing and querying static SDE data.
- `crates/worker`: sync status and worker boundary shell.

The desktop app lives in `apps/desktop`:

- `apps/desktop/src`: React UI and typed Tauri command wrappers.
- `apps/desktop/src-tauri`: Tauri 2 Rust crate and command handlers.

React renders prepared views and calls Tauri commands. Tauri commands are adapters over Rust crates; trading calculations should stay in `crates/domain`.

## MVP Surfaces

The first desktop screen exposes three surfaces:

- `Market Price Lookup`: lookup current Jita price state for an item.
- `Selection Discovery`: list candidate items with entry, exit, net profit, scores, and reasons.
- `Order Monitor`: list active-order-style fixture rows with recommended action and urgency.

Sync status is split between public and private flows:

- Public market sync: `live-ready`, `fixture-ready`, or `fixture-fallback`
- Authenticated order sync: `not-authorized`
- Data source: `live` or `fixture`

## Scope

In scope for this foundation:

- Local Tauri desktop shell.
- Public ESI-backed market lookup and selection discovery.
- Fixture fallback command boundary.
- React UI wired to Tauri commands.
- Deterministic, testable Rust domain calculations.

Out of scope for this foundation:

- Full trading persistence beyond the Supabase static SDE catalog.
- EVE SSO token handling.
- Authenticated character order synchronization.
- Automated market order placement, modification, or cancellation.
