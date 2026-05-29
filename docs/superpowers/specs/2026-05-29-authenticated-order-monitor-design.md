# Authenticated Order Monitor Design

Date: 2026-05-29

## Purpose

Build the first real Order Monitor slice for station traders: users authorize EveTools with EVE SSO, the backend syncs their open character market orders, and the desktop monitor compares those orders with the existing public market snapshots to recommend whether to raise, lower, or hold each order.

This feature replaces fixture-only Order Monitor data for authorized users. It remains advisory only. EveTools must not place, modify, or cancel EVE orders in this phase.

## Scope

In scope:

- EVE SSO authorization with PKCE for a native desktop app.
- Required scope: `esi-markets.read_character_orders.v1`.
- Backend-only token handling and refresh.
- Character identity storage.
- Authenticated ESI client support for `GET /characters/{character_id}/orders/`.
- Postgres persistence for characters, auth tokens, authenticated sync runs, and character order snapshots.
- Worker/API methods to sync open orders and read analyzed order monitor rows.
- Desktop commands and UI actions for login, sync, auth status, and order monitor display.
- Repricing advice based on the current order price, latest public station order book, and order side.

Out of scope:

- Automatic order placement, modification, or cancellation.
- Wallet, assets, transactions, and realized profit analysis.
- Corporation orders.
- Multi-character switching beyond storing more than one authorized character safely.
- Long-term secret hardening beyond keeping tokens out of React/WebView state and database documentation.

## External Contracts

EVE SSO is the OAuth provider. The desktop app uses PKCE and opens the authorization URL in the system browser. The callback is handled by a local Rust listener on `127.0.0.1`; the redirect URI must match the URI configured in the EVE developer application.

The EVE developer app configuration is supplied by environment variables:

- `EVETOOLS_SSO_CLIENT_ID`
- `EVETOOLS_SSO_REDIRECT_URI`

The initial redirect URI should use a loopback callback such as `http://127.0.0.1:17813/callback`. The implementation validates the returned `state` before exchanging the authorization code.

Official references:

- EVE SSO: https://developers.eveonline.com/docs/services/sso/
- Native SSO with PKCE: https://docs.esi.evetech.net/docs/sso/native_sso_flow.html
- ESI swagger: https://esi.evetech.net/latest/swagger.json

## Architecture

The existing boundary stays intact:

```text
React UI
  -> Tauri commands
  -> Rust auth/order services
  -> ESI client + DB repositories
  -> Domain repricing analysis
```

React never sees access tokens or refresh tokens. Tauri commands expose coarse actions and views:

- Start SSO authorization.
- Complete or observe authorization result.
- Read auth status.
- Trigger character-order sync.
- Read Order Monitor rows.

The ESI crate owns authenticated HTTP calls and OAuth token exchange. The DB crate owns schema and repositories. The worker crate owns sync orchestration. The API crate owns read-side analysis for order monitor rows. The domain crate owns deterministic repricing classification.

## Data Model

Add a new migration after `0004_add_market_sync_operations.sql`.

Tables:

- `evetools_catalog.characters`
  - `character_id BIGINT PRIMARY KEY`
  - `character_name TEXT NOT NULL`
  - `owner_hash TEXT`
  - `last_login_at TIMESTAMPTZ NOT NULL`
  - `updated_at TIMESTAMPTZ NOT NULL`

- `evetools_catalog.character_auth_tokens`
  - `character_id BIGINT PRIMARY KEY REFERENCES characters`
  - `refresh_token TEXT NOT NULL`
  - `access_token TEXT`
  - `access_token_expires_at TIMESTAMPTZ`
  - `scopes TEXT[] NOT NULL`
  - `token_type TEXT NOT NULL`
  - `updated_at TIMESTAMPTZ NOT NULL`

- `evetools_catalog.character_order_sync_runs`
  - `sync_run_id BIGSERIAL PRIMARY KEY`
  - `character_id BIGINT NOT NULL REFERENCES characters`
  - `started_at TIMESTAMPTZ NOT NULL`
  - `completed_at TIMESTAMPTZ`
  - `status TEXT NOT NULL`
  - `order_count BIGINT`
  - `error_summary TEXT`

- `evetools_catalog.character_order_snapshots`
  - `sync_run_id BIGINT NOT NULL REFERENCES character_order_sync_runs`
  - `character_id BIGINT NOT NULL`
  - `order_id BIGINT NOT NULL`
  - `type_id INTEGER NOT NULL`
  - `region_id INTEGER NOT NULL`
  - `location_id BIGINT NOT NULL`
  - `is_buy_order BOOLEAN NOT NULL`
  - `price DOUBLE PRECISION NOT NULL`
  - `volume_remain BIGINT NOT NULL`
  - `volume_total BIGINT NOT NULL`
  - `issued TEXT NOT NULL`
  - `duration INTEGER NOT NULL`
  - `min_volume INTEGER`
  - `order_range TEXT NOT NULL`
  - `is_corporation BOOLEAN NOT NULL`
  - `escrow DOUBLE PRECISION`

Only the latest successful character-order sync is used for the monitor view. Historical sync rows remain available for diagnostics.

## SSO Flow

1. Desktop command validates SSO config.
2. Backend creates a PKCE verifier/challenge and random state.
3. Backend opens the EVE authorization URL in the system browser.
4. Local callback listener receives `code` and `state`.
5. Backend rejects mismatched state or callback errors.
6. Backend exchanges code for tokens at EVE SSO.
7. Backend validates identity from the token payload or verification endpoint.
8. Backend stores character metadata and refresh token.
9. Backend returns an auth status view without secrets.

The first implementation may keep only one active authorization session per desktop process. If a second login is started while one is active, return a clear error.

## Character Order Sync

The sync service:

1. Loads the selected character token.
2. Refreshes the access token if needed.
3. Calls `GET /characters/{character_id}/orders/`.
4. Starts a character-order sync run.
5. Replaces snapshots for that sync run.
6. Marks the sync run successful or failed.

The endpoint is cached by ESI for up to 1200 seconds. The UI should show sync time and avoid implying sub-minute freshness.

## Repricing Analysis

For each character order:

- Match by `region_id`, `location_id`, and `type_id` against the latest public station order book.
- Sell order:
  - If order price is above best ask, recommend `lower` to one price step below best ask.
  - If order is at or below best ask, recommend `hold`.
- Buy order:
  - If order price is below best bid, recommend `raise` to one price step above best bid.
  - If order is at or above best bid, recommend `hold`.
- If public market data is missing, recommend `hold` with a stale or missing data reason.

The first price step can be a simple ISK 0.01 adjustment for decimal prices. More exact EVE tick-size rules are deferred but must be isolated in a domain helper so they can be replaced later.

Suggested reason codes:

- `undercut_detected`
- `overbid_detected`
- `already_best_price`
- `missing_market_data`
- `stale_market_data`
- `authenticated_sync_required`

## Error Handling

Handle these explicitly:

- Missing SSO client config.
- Browser open failure.
- SSO cancellation or callback error.
- State mismatch.
- Token exchange failure.
- Token refresh failure.
- Missing required scope.
- Character orders 401/403.
- Empty order list.
- Missing public market snapshot for an order.

Errors returned to the UI must not include access tokens, refresh tokens, or full authorization URLs with codes.

## Testing

Use TDD for each slice:

- Unit tests for PKCE verifier/challenge shape and auth URL construction.
- HTTP mock tests for OAuth token exchange and character order ESI calls.
- Parser tests for character order response payloads.
- DB repository tests for characters, tokens, sync runs, and order snapshots.
- Domain tests for sell/buy repricing decisions.
- API tests for analyzed order monitor rows using fixture character orders and public snapshots.
- Tauri command tests for auth status and fixture-safe failure paths.
- TypeScript typecheck for new UI command wrappers and UI state.

## Security Rules

- Never place refresh tokens in React state or TypeScript types.
- Never log access tokens, refresh tokens, authorization codes, or callback URLs containing codes.
- Do not document real EVE client IDs as examples.
- Keep token persistence in server-side/Rust-owned storage.
- Prefer short-lived access-token reuse only until `access_token_expires_at`.

## Success Criteria

This slice is complete when a configured desktop app can authorize a character, sync that character's open orders from ESI, persist the snapshot, and show real Order Monitor rows with advisory repricing actions based on the current public market snapshot.
