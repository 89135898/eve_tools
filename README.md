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

### Database Connection

Set the catalog database URL in your local shell before starting the desktop app:

```bash
export EVETOOLS_DATABASE_URL="<supabase-postgres-url-with-sslmode-require>"
pnpm dev
```

Use a connection string from the Supabase Dashboard's `Connect` panel. See Supabase's [database connection guide](https://supabase.com/docs/guides/database/connecting-to-postgres/serverless-drivers) when choosing between direct connections and poolers. For catalog import work, prefer one of these connection modes:

- Direct Postgres connection with SSL enabled. This is the preferred mode for local/admin imports because the importer runs migrations and long transactions.
- Supavisor session pooler if your local network cannot reach the direct IPv6 endpoint.
- Do not use the transaction pooler for this importer. The importer uses long transactions and `sqlx`; transaction pooling is intended for short-lived/serverless traffic and can conflict with prepared-statement behavior.

The URL must include SSL. Use `?sslmode=require` when there are no query parameters, or append `&sslmode=require` if the URL already has query parameters. If you configure Supabase SSL enforcement and install the project CA certificate locally, `sslmode=verify-full` is stronger.

For repository integration tests that should touch Postgres, set a separate test URL:

```bash
export EVETOOLS_TEST_DATABASE_URL="<dev-or-test-supabase-postgres-url-with-sslmode-require>"
cargo test -p evetools-db --test catalog_repository -- --nocapture
```

When `EVETOOLS_TEST_DATABASE_URL` is not set, Postgres integration tests skip themselves. The importer owns the `evetools_catalog` schema and replaces catalog rows for each successful import, so use a development or disposable Supabase project for tests.

Do not commit real database URLs or passwords. Do not store them in checked-in `.env` files. If a credential is pasted into chat, logs, screenshots, or source control, rotate it in Supabase before use.

Direct Supabase Postgres mode is only for local, private, or admin catalog imports.
`EVETOOLS_DATABASE_URL` is a privileged credential: do not bundle it into the Tauri app,
inject it for end users, or require end users to hold it. Before production distribution,
replace direct database access with a hosted API, Supabase Edge Function, or a strictly
RLS-enforced read-only path.

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
