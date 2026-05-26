# EveTools

EveTools is a desktop-first EVE Online station trading assistant focused on Jita 4-4.

The current implementation slice provides:

- Tauri 2 desktop shell
- React/Vite desktop UI
- Rust workspace crates for domain logic, ESI, storage, and workers
- Tested Rust domain calculations for spread, fees, liquidity, and attention scoring
- Fixture-backed market price lookup
- Fixture-backed selection discovery
- Fixture-backed order monitor

This slice intentionally uses deterministic fixture data. Real public ESI sync, SQLite persistence, EVE SSO, and authenticated character-order sync are deferred to later implementation phases.

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

## Architecture

Business logic lives in Rust crates:

- `crates/domain`: market models, price calculations, scoring, serialized view models, and fixtures.
- `crates/esi`: ESI client boundary shell.
- `crates/db`: storage boundary shell.
- `crates/worker`: sync status and worker boundary shell.

The desktop app lives in `apps/desktop`:

- `apps/desktop/src`: React UI and typed Tauri command wrappers.
- `apps/desktop/src-tauri`: Tauri 2 Rust crate and command handlers.

React renders prepared views and calls Tauri commands. Tauri commands are adapters over Rust crates; trading calculations should stay in `crates/domain`.

## MVP Surfaces

The first desktop screen exposes three fixture-backed surfaces:

- `Market Price Lookup`: lookup current Jita price state for an item.
- `Selection Discovery`: list candidate items with entry, exit, net profit, scores, and reasons.
- `Order Monitor`: list active-order-style fixture rows with recommended action and urgency.

Fixture sync status is split between public and private flows:

- Public market sync: `fixture-ready`
- Authenticated order sync: `not-authorized`

## Scope

In scope for this foundation:

- Local Tauri desktop shell.
- Fixture-backed command boundary.
- React UI wired to Tauri commands.
- Deterministic, testable Rust domain calculations.

Out of scope for this foundation:

- Real ESI HTTP calls.
- SQLite schema and repositories.
- EVE SSO token handling.
- Authenticated character order synchronization.
- Automated market order placement, modification, or cancellation.
