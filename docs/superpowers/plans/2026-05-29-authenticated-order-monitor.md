# Authenticated Order Monitor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add real EVE SSO authorization and authenticated character-order synchronization so Order Monitor can display advisory repricing rows for the user's open market orders.

**Architecture:** Extend existing Rust boundaries rather than adding a new service layer. `crates/esi` owns OAuth and authenticated ESI HTTP calls; `crates/db` owns character/token/order persistence; `crates/domain` owns repricing analysis; `crates/worker` orchestrates authenticated sync; `crates/api`, `crates/http-api`, and Tauri commands expose safe read/action views to React.

**Tech Stack:** Rust 1.82+, Tokio, reqwest, serde, SQLx/Postgres, Axum, Tauri 2, React/Vite/TypeScript, EVE SSO PKCE, ESI `esi-markets.read_character_orders.v1`.

---

## File Map

- Modify `Cargo.toml`: add workspace dependencies needed for PKCE/JWT-safe helpers if not already present.
- Modify `crates/esi/src/models.rs`: add OAuth token and character order response models.
- Modify `crates/esi/src/client.rs`: add token exchange, refresh, and authenticated character orders.
- Modify `crates/esi/src/lib.rs`: export new types and errors.
- Add `crates/esi/tests/fixtures/character_orders.json`.
- Modify `crates/esi/tests/model_parsing.rs` and `crates/esi/tests/client.rs`.
- Add `crates/db/migrations/0005_add_authenticated_order_monitor.sql`.
- Modify `crates/db/src/lib.rs`, `crates/db/src/schema.rs`.
- Add `crates/db/src/auth.rs`.
- Add `crates/db/tests/auth_repository.rs`.
- Modify `crates/domain/src/views.rs`, `crates/domain/src/fixtures.rs`, `crates/domain/src/lib.rs`.
- Add `crates/domain/src/repricing.rs`.
- Modify `crates/api/src/lib.rs` and `crates/api/tests/read_api.rs`.
- Modify `crates/http-api/src/lib.rs` and `crates/http-api/tests/read_http_api.rs`.
- Modify `crates/worker/src/lib.rs` and add authenticated sync tests.
- Modify `apps/desktop/src-tauri/src/lib.rs`.
- Modify `apps/desktop/src/commands.ts`, `apps/desktop/src/App.tsx`, `apps/desktop/src/i18n/resources.ts`, `apps/desktop/src/styles.css`.
- Modify `README.md`.

## Task 1: ESI OAuth and Character Order Client

**Files:**
- Modify: `crates/esi/src/models.rs`
- Modify: `crates/esi/src/client.rs`
- Modify: `crates/esi/src/lib.rs`
- Modify: `crates/esi/tests/model_parsing.rs`
- Modify: `crates/esi/tests/client.rs`
- Create: `crates/esi/tests/fixtures/character_orders.json`

- [ ] **Step 1: Add failing model parsing tests**

Add `character_orders.json`:

```json
[
  {
    "duration": 90,
    "escrow": 0.0,
    "is_buy_order": false,
    "is_corporation": false,
    "issued": "2026-05-29T10:00:00Z",
    "location_id": 60003760,
    "min_volume": 1,
    "order_id": 8000000001,
    "price": 5.60,
    "range": "station",
    "region_id": 10000002,
    "type_id": 34,
    "volume_remain": 100000,
    "volume_total": 200000
  },
  {
    "duration": 90,
    "escrow": 120000.0,
    "is_buy_order": true,
    "is_corporation": false,
    "issued": "2026-05-29T10:05:00Z",
    "location_id": 60003760,
    "min_volume": 1,
    "order_id": 8000000002,
    "price": 4.95,
    "range": "station",
    "region_id": 10000002,
    "type_id": 34,
    "volume_remain": 50000,
    "volume_total": 100000
  }
]
```

Add tests that parse `Vec<EsiCharacterOrder>` and `EsiTokenResponse`.

- [ ] **Step 2: Verify RED**

Run: `cargo test -p evetools-esi --test model_parsing parses_character_orders_response parses_token_response`

Expected: fail with unresolved imports.

- [ ] **Step 3: Implement models and exports**

Add `EsiCharacterOrder`, `EsiTokenResponse`, `EsiCharacterIdentity`, and export them.

- [ ] **Step 4: Add failing authenticated client tests**

Use `httpmock` to verify:

- `exchange_authorization_code()` posts form data to `/v2/oauth/token`.
- `refresh_access_token()` posts refresh grant.
- `character_orders()` calls `/latest/characters/{character_id}/orders/` with bearer auth.

- [ ] **Step 5: Verify RED**

Run: `cargo test -p evetools-esi --test client character_orders_use_bearer_token exchanges_authorization_code_with_pkce refreshes_access_token`

Expected: fail because methods do not exist.

- [ ] **Step 6: Implement OAuth and authenticated ESI methods**

Add methods to `EsiClient` using existing `reqwest::Client`:

- `exchange_authorization_code(base_sso_url, client_id, code, redirect_uri, code_verifier)`
- `refresh_access_token(base_sso_url, client_id, refresh_token)`
- `character_orders(character_id, access_token)`

- [ ] **Step 7: Verify GREEN**

Run: `cargo test -p evetools-esi`

Expected: pass.

- [ ] **Step 8: Commit**

Run:

```bash
git add crates/esi
git commit -m "feat: add authenticated esi order client"
```

## Task 2: Auth Persistence

**Files:**
- Create: `crates/db/migrations/0005_add_authenticated_order_monitor.sql`
- Modify: `crates/db/src/schema.rs`
- Create: `crates/db/src/auth.rs`
- Modify: `crates/db/src/lib.rs`
- Create: `crates/db/tests/auth_repository.rs`

- [ ] **Step 1: Write failing schema and repository tests**

Tests cover:

- migration 5 is registered and contains character/auth/order tables.
- upsert character and token.
- start/complete/fail character order sync runs.
- replace and read latest successful character order snapshots.

- [ ] **Step 2: Verify RED**

Run: `cargo test -p evetools-db auth_repository schema::tests::adds_authenticated_order_monitor_tables -- --nocapture`

Expected: fail because migration and repository are missing.

- [ ] **Step 3: Add migration and repository**

Create `AuthRepository` with types:

- `AuthorizedCharacter`
- `CharacterAuthToken`
- `CharacterOrderSnapshotInput`
- `CharacterOrderSnapshot`
- `CharacterOrderSyncSummary`

Keep all SQL `.persistent(false)`.

- [ ] **Step 4: Verify GREEN**

Run: `cargo test -p evetools-db --test auth_repository -- --nocapture`

Expected: pass or skip only when local Postgres URL is not configured, matching existing integration behavior.

- [ ] **Step 5: Commit**

Run:

```bash
git add crates/db
git commit -m "feat: store authenticated character orders"
```

## Task 3: Repricing Domain Analysis

**Files:**
- Create: `crates/domain/src/repricing.rs`
- Modify: `crates/domain/src/views.rs`
- Modify: `crates/domain/src/fixtures.rs`
- Modify: `crates/domain/src/lib.rs`

- [ ] **Step 1: Write failing repricing tests**

Tests cover:

- sell order above best ask recommends `lower`.
- buy order below best bid recommends `raise`.
- already-best orders recommend `hold`.
- missing public market data recommends `hold` with `missing_market_data`.

- [ ] **Step 2: Verify RED**

Run: `cargo test -p evetools-domain repricing`

Expected: fail because module/types are missing.

- [ ] **Step 3: Implement repricing helpers**

Add deterministic helper using a simple `0.01` ISK step:

- `analyze_character_order()`
- `recommended_action`
- `recommended_price`
- `urgency_score`
- reason codes.

- [ ] **Step 4: Update `OrderMonitorView`**

Extend view fields only if needed while preserving existing UI fields.

- [ ] **Step 5: Verify GREEN**

Run: `cargo test -p evetools-domain`

Expected: pass.

- [ ] **Step 6: Commit**

Run:

```bash
git add crates/domain
git commit -m "feat: analyze character order repricing"
```

## Task 4: Worker Auth Sync

**Files:**
- Modify: `crates/worker/src/lib.rs`
- Modify: `crates/worker/tests/public_market_sync.rs` or add `crates/worker/tests/authenticated_order_sync.rs`

- [ ] **Step 1: Write failing worker tests**

Use mock ESI to:

- refresh access token when expired.
- fetch character orders.
- write a successful sync run and snapshots.
- record failed sync without leaking token text.

- [ ] **Step 2: Verify RED**

Run: `cargo test -p evetools-worker authenticated_order_sync -- --nocapture`

Expected: fail because sync function is missing.

- [ ] **Step 3: Implement sync function**

Add `sync_authenticated_character_orders(repository, client, sso_base_url, client_id, character_id)` and summary type.

- [ ] **Step 4: Verify GREEN**

Run: `cargo test -p evetools-worker authenticated_order_sync -- --nocapture`

Expected: pass.

- [ ] **Step 5: Commit**

Run:

```bash
git add crates/worker
git commit -m "feat: sync authenticated character orders"
```

## Task 5: Read API and HTTP API

**Files:**
- Modify: `crates/api/src/lib.rs`
- Modify: `crates/api/tests/read_api.rs`
- Modify: `crates/http-api/src/lib.rs`
- Modify: `crates/http-api/tests/read_http_api.rs`

- [ ] **Step 1: Write failing read API tests**

Seed public station orders and character order snapshots, then assert `order_monitor_items(character_id, language)` returns real rows with `lower` and `raise` actions.

- [ ] **Step 2: Verify RED**

Run: `cargo test -p evetools-api --test read_api order_monitor`

Expected: fail because API method is missing.

- [ ] **Step 3: Implement read API method**

Join latest character orders with latest public station order books and call domain repricing helpers.

- [ ] **Step 4: Add HTTP route**

Add `GET /characters/{character_id}/order-monitor?language=zh-CN`.

- [ ] **Step 5: Verify GREEN**

Run:

```bash
cargo test -p evetools-api --test read_api
cargo test -p evetools-http-api --test read_http_api
```

Expected: pass.

- [ ] **Step 6: Commit**

Run:

```bash
git add crates/api crates/http-api
git commit -m "feat: expose authenticated order monitor api"
```

## Task 6: Tauri SSO Commands

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: Write failing Tauri command tests**

Test:

- missing SSO config reports a safe error.
- auth status returns not-authorized when no token exists.
- sync command reports missing hosted API or missing auth without leaking secrets.

- [ ] **Step 2: Verify RED**

Run: `cargo test -p evetools-desktop auth`

Expected: fail because commands are missing.

- [ ] **Step 3: Implement commands**

Add:

- `get_auth_status`
- `start_eve_sso_login`
- `sync_character_orders`
- `list_order_monitor_items` reads hosted authenticated monitor when authorized, fixture only when explicitly fixture mode or no auth.

If system-browser opening is unavailable in tests, isolate it behind a small helper.

- [ ] **Step 4: Verify GREEN**

Run: `cargo test -p evetools-desktop`

Expected: pass.

- [ ] **Step 5: Commit**

Run:

```bash
git add apps/desktop/src-tauri
git commit -m "feat: add desktop eve sso commands"
```

## Task 7: Desktop UI

**Files:**
- Modify: `apps/desktop/src/commands.ts`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/i18n/resources.ts`
- Modify: `apps/desktop/src/styles.css`

- [ ] **Step 1: Add TypeScript command wrappers**

Add auth status, login, sync, and real order monitor wrappers.

- [ ] **Step 2: Update UI**

Order Monitor shows:

- auth status.
- login button.
- sync orders button.
- last sync/authorized character where available.
- existing order table with real rows.

- [ ] **Step 3: Add translations**

Add zh-CN/en-US strings for login, sync, authorized, missing auth, and new reason codes.

- [ ] **Step 4: Verify typecheck**

Run: `pnpm --filter @evetools/desktop typecheck`

Expected: pass.

- [ ] **Step 5: Commit**

Run:

```bash
git add apps/desktop/src
git commit -m "feat: add order monitor auth controls"
```

## Task 8: Documentation and Final Verification

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Document SSO configuration**

Document:

- `EVETOOLS_SSO_CLIENT_ID`
- `EVETOOLS_SSO_REDIRECT_URI`
- scope `esi-markets.read_character_orders.v1`
- token security boundaries
- no automatic order modification.

- [ ] **Step 2: Run full verification**

Run:

```bash
cargo fmt --all -- --check
cargo test --workspace
pnpm --filter @evetools/desktop typecheck
git status --short
```

Expected: pass, with only intentional README changes before final commit.

- [ ] **Step 3: Commit**

Run:

```bash
git add README.md
git commit -m "docs: document eve sso order monitor"
```

## Self-Review

- Spec coverage: plan covers SSO, token handling, authenticated ESI orders, persistence, sync, analysis, API, Tauri commands, UI, and docs.
- Placeholders: none; every task has a concrete file set and verification commands.
- Scope: automatic order changes, wallet, corporation orders, and multi-character UX are excluded.
