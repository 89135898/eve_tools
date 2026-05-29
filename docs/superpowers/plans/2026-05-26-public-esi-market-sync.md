# Public ESI Market Sync Implementation Plan

> Status update, 2026-05-29: this is a historical implementation plan for the first live public ESI slice. The current implementation uses Supabase/Postgres snapshots, a hosted HTTP read API, multi-hub Selection Discovery, strict live-mode errors instead of implicit fixture fallback, and a separate completed EVE SSO authenticated Order Monitor path. Use `README.md` and the current specs for the up-to-date runtime shape.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first live public ESI vertical slice for Jita market price lookup and selection discovery while preserving deterministic fixture fallback.

**Architecture:** `crates/esi` owns typed ESI HTTP access and response parsing. `crates/domain` owns deterministic Jita order-book aggregation, trend classification, and selection scoring. `apps/desktop/src-tauri` stays a thin async adapter that validates command input, calls the ESI-backed service, and falls back to fixtures when public ESI is unavailable.

**Tech Stack:** Rust 1.82 workspace, Tauri 2 commands, `reqwest` with Rustls, `serde`, `rust_decimal`, `chrono`, `tokio`, `httpmock` for ESI client tests, React/Vite/i18next for code-label display.

---

## Scope

This plan implements public market sync only:

- Live lookup for a specific EVE inventory type by exact name or numeric type id.
- Live Jita 4-4 price summary from public The Forge market orders.
- Live daily volume and price trend from public market history.
- Live selection discovery over a small fixed seed pool.
- Fixture fallback for local development and ESI outages.

This plan does not implement:

- EVE SSO.
- Character orders.
- SQLite persistence.
- Background scheduling.
- Automated order placement, modification, or cancellation.
- Full universe-wide item crawling.

Official ESI endpoints used:

- `POST /universe/ids/` for exact name-to-type-id resolution.
- `GET /universe/types/{type_id}/` for type metadata when the query is numeric.
- `GET /markets/{region_id}/orders/` with `order_type=all`, `type_id`, and paginated responses.
- `GET /markets/{region_id}/history/` with `type_id`.

Constants already exist in `crates/domain/src/market.rs`:

- The Forge region id: `10000002`.
- Jita 4-4 station id: `60003760`.

## File Structure

- Modify `Cargo.toml`: add workspace-level `reqwest`.
- Modify `crates/esi/Cargo.toml`: add `reqwest`, `serde`, `serde_json`, and `httpmock` test dependency.
- Replace `crates/esi/src/lib.rs`: export the client, models, and richer errors.
- Create `crates/esi/src/models.rs`: typed ESI response models and request enums.
- Create `crates/esi/src/client.rs`: public ESI client with pagination and type resolution.
- Create `crates/esi/tests/fixtures/*.json`: deterministic ESI payloads used by parsing tests.
- Modify `crates/domain/src/lib.rs`: export new analysis functions and types.
- Modify `crates/domain/src/views.rs`: add conversion constructors from domain summaries.
- Create `crates/domain/src/market_analysis.rs`: Jita order-book aggregation, history trend, candidate scoring.
- Modify `apps/desktop/src-tauri/Cargo.toml`: depend on `evetools-esi`, `chrono`, `rust_decimal`, and `tokio`.
- Modify `apps/desktop/src-tauri/src/lib.rs`: convert commands to async and route through live-or-fixture public market service.
- Modify `crates/worker/src/lib.rs`: expose public market sync status codes for live and fixture modes.
- Modify `apps/desktop/src/i18n/resources.ts`: add labels for new sync status, data source, trend, and reason codes.
- Modify `apps/desktop/src/App.tsx`: show live/fixture data source from backend status rather than hard-coded fixture.
- Modify `apps/desktop/src/commands.ts`: extend `SyncStatus` with `data_source`.
- Modify `README.md`: document public ESI mode, fixture fallback, and verification commands.

## Public Source Mode

Use an environment variable for deterministic local behavior:

```text
EVETOOLS_MARKET_SOURCE=live
EVETOOLS_MARKET_SOURCE=fixture
```

Rules:

- Missing variable means `live`.
- `fixture` never calls ESI and returns current fixtures.
- `live` calls public ESI.
- Network, HTTP, and decode failures in `live` mode fall back to fixtures and report `public_market_sync = "fixture-fallback"`.
- Item-not-found errors do not fall back silently; the lookup command returns `"Item not found"`.
- Selection discovery falls back to fixture candidates if every live candidate fails.
- The Tauri adapter stores the latest public-market fallback signal in process memory; durable sync-run history belongs to the later SQLite phase.

## Task 1: Add ESI Models and Fixture Parsing Tests

**Files:**

- Modify: `Cargo.toml`
- Modify: `crates/esi/Cargo.toml`
- Replace: `crates/esi/src/lib.rs`
- Create: `crates/esi/src/models.rs`
- Create: `crates/esi/tests/model_parsing.rs`
- Create: `crates/esi/tests/fixtures/market_orders.json`
- Create: `crates/esi/tests/fixtures/market_history.json`
- Create: `crates/esi/tests/fixtures/universe_ids.json`
- Create: `crates/esi/tests/fixtures/type_info.json`

- [ ] **Step 1: Write ESI fixture files**

Create `crates/esi/tests/fixtures/market_orders.json`:

```json
[
  {
    "duration": 90,
    "is_buy_order": true,
    "issued": "2026-05-25T11:45:00Z",
    "location_id": 60003760,
    "min_volume": 1,
    "order_id": 7000000001,
    "price": 5.01,
    "range": "station",
    "system_id": 30000142,
    "type_id": 34,
    "volume_remain": 500000,
    "volume_total": 1000000
  },
  {
    "duration": 90,
    "is_buy_order": false,
    "issued": "2026-05-25T11:46:00Z",
    "location_id": 60003760,
    "min_volume": 1,
    "order_id": 7000000002,
    "price": 5.49,
    "range": "station",
    "system_id": 30000142,
    "type_id": 34,
    "volume_remain": 620000,
    "volume_total": 800000
  }
]
```

Create `crates/esi/tests/fixtures/market_history.json`:

```json
[
  {
    "average": 5.10,
    "date": "2026-05-24",
    "highest": 5.30,
    "lowest": 4.90,
    "order_count": 250,
    "volume": 1000000
  },
  {
    "average": 5.18,
    "date": "2026-05-25",
    "highest": 5.60,
    "lowest": 5.00,
    "order_count": 280,
    "volume": 1250000
  }
]
```

Create `crates/esi/tests/fixtures/universe_ids.json`:

```json
{
  "inventory_types": [
    {
      "id": 34,
      "name": "Tritanium"
    }
  ]
}
```

Create `crates/esi/tests/fixtures/type_info.json`:

```json
{
  "capacity": 0,
  "description": "The main building block in space structures.",
  "group_id": 18,
  "market_group_id": 1857,
  "mass": 0,
  "name": "Tritanium",
  "packaged_volume": 0.01,
  "portion_size": 1,
  "published": true,
  "radius": 1,
  "type_id": 34,
  "volume": 0.01
}
```

- [ ] **Step 2: Add model parsing tests**

Create `crates/esi/tests/model_parsing.rs`:

```rust
use evetools_esi::{
    EsiMarketHistoryDay, EsiMarketOrder, EsiTypeInfo, UniverseIdsResponse,
};

#[test]
fn parses_market_orders_response() {
    let json = include_str!("fixtures/market_orders.json");
    let orders: Vec<EsiMarketOrder> = serde_json::from_str(json).unwrap();

    assert_eq!(orders.len(), 2);
    assert_eq!(orders[0].type_id, 34);
    assert!(orders[0].is_buy_order);
    assert_eq!(orders[0].location_id, 60003760);
    assert_eq!(orders[0].volume_remain, 500_000);
    assert_eq!(orders[1].price, 5.49);
}

#[test]
fn parses_market_history_response() {
    let json = include_str!("fixtures/market_history.json");
    let history: Vec<EsiMarketHistoryDay> = serde_json::from_str(json).unwrap();

    assert_eq!(history.len(), 2);
    assert_eq!(history[1].date, "2026-05-25");
    assert_eq!(history[1].volume, 1_250_000);
    assert_eq!(history[1].average, 5.18);
}

#[test]
fn parses_universe_ids_response() {
    let json = include_str!("fixtures/universe_ids.json");
    let response: UniverseIdsResponse = serde_json::from_str(json).unwrap();

    let entry = response.inventory_types.unwrap().remove(0);
    assert_eq!(entry.id, 34);
    assert_eq!(entry.name, "Tritanium");
}

#[test]
fn parses_type_info_response() {
    let json = include_str!("fixtures/type_info.json");
    let response: EsiTypeInfo = serde_json::from_str(json).unwrap();

    assert_eq!(response.type_id, 34);
    assert_eq!(response.name, "Tritanium");
    assert!(response.published);
    assert_eq!(response.market_group_id, Some(1857));
}
```

- [ ] **Step 3: Run parsing tests and verify they fail before models exist**

Run:

```bash
cargo test -p evetools-esi --test model_parsing
```

Expected: FAIL with unresolved imports for `EsiMarketHistoryDay`, `EsiMarketOrder`, `EsiTypeInfo`, and `UniverseIdsResponse`.

- [ ] **Step 4: Add dependencies**

Modify root `Cargo.toml` workspace dependencies:

```toml
[workspace.dependencies]
anyhow = "1"
chrono = { version = "0.4", features = ["serde"] }
reqwest = { version = "0.13", default-features = false, features = ["json", "rustls-tls"] }
rust_decimal = { version = "1", features = ["serde"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tauri = { version = "2" }
tauri-build = { version = "2" }
thiserror = "2"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
```

Modify `crates/esi/Cargo.toml`:

```toml
[dependencies]
reqwest.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true

[dev-dependencies]
httpmock = "0.8.3"
tokio.workspace = true
```

- [ ] **Step 5: Implement ESI models and exports**

Replace `crates/esi/src/lib.rs`:

```rust
pub mod client;
pub mod models;

use thiserror::Error;

pub use client::EsiClient;
pub use models::{
    EsiMarketHistoryDay, EsiMarketOrder, EsiOrderType, EsiTypeInfo, ResolvedInventoryType,
    UniverseIdEntry, UniverseIdsResponse,
};

#[derive(Debug, Error)]
pub enum EsiError {
    #[error("ESI HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("ESI response could not be decoded: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("Item not found")]
    ItemNotFound,
    #[error("ESI returned status {status}: {body}")]
    Status { status: u16, body: String },
    #[error("Invalid inventory type id: {0}")]
    InvalidTypeId(String),
}

pub fn client_mode() -> &'static str {
    "live"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esi_crate_reports_live_client_mode() {
        assert_eq!(client_mode(), "live");
        assert_eq!(EsiError::ItemNotFound.to_string(), "Item not found");
    }
}
```

Create `crates/esi/src/models.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EsiOrderType {
    All,
    Buy,
    Sell,
}

impl EsiOrderType {
    pub fn as_query_value(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Buy => "buy",
            Self::Sell => "sell",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EsiMarketOrder {
    pub duration: i32,
    pub is_buy_order: bool,
    pub issued: String,
    pub location_id: i64,
    pub min_volume: i32,
    pub order_id: i64,
    pub price: f64,
    pub range: String,
    pub system_id: i32,
    pub type_id: i32,
    pub volume_remain: i32,
    pub volume_total: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EsiMarketHistoryDay {
    pub average: f64,
    pub date: String,
    pub highest: f64,
    pub lowest: f64,
    pub order_count: i64,
    pub volume: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EsiTypeInfo {
    pub group_id: i32,
    pub market_group_id: Option<i32>,
    pub name: String,
    pub published: bool,
    pub type_id: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UniverseIdsResponse {
    pub inventory_types: Option<Vec<UniverseIdEntry>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UniverseIdEntry {
    pub id: i32,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedInventoryType {
    pub type_id: i32,
    pub name: String,
}
```

Create `crates/esi/src/client.rs` with a skeleton that compiles before HTTP behavior is added:

```rust
use crate::{
    EsiError, EsiMarketHistoryDay, EsiMarketOrder, EsiOrderType, EsiTypeInfo,
    ResolvedInventoryType, UniverseIdsResponse,
};

#[derive(Clone, Debug)]
pub struct EsiClient {
    base_url: String,
    http: reqwest::Client,
}

impl EsiClient {
    pub fn tranquility() -> Self {
        Self::new("https://esi.evetech.net")
    }

    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn resolve_inventory_type(
        &self,
        query: &str,
    ) -> Result<ResolvedInventoryType, EsiError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Err(EsiError::ItemNotFound);
        }

        if let Ok(type_id) = trimmed.parse::<i32>() {
            let info = self.type_info(type_id).await?;
            return Ok(ResolvedInventoryType {
                type_id: info.type_id,
                name: info.name,
            });
        }

        let ids = self.universe_ids(trimmed).await?;
        let entry = ids
            .inventory_types
            .unwrap_or_default()
            .into_iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(trimmed))
            .ok_or(EsiError::ItemNotFound)?;

        Ok(ResolvedInventoryType {
            type_id: entry.id,
            name: entry.name,
        })
    }

    pub async fn universe_ids(&self, _name: &str) -> Result<UniverseIdsResponse, EsiError> {
        Err(EsiError::ItemNotFound)
    }

    pub async fn type_info(&self, _type_id: i32) -> Result<EsiTypeInfo, EsiError> {
        Err(EsiError::ItemNotFound)
    }

    pub async fn market_orders(
        &self,
        _region_id: i32,
        _type_id: i32,
        _order_type: EsiOrderType,
    ) -> Result<Vec<EsiMarketOrder>, EsiError> {
        Ok(Vec::new())
    }

    pub async fn market_history(
        &self,
        _region_id: i32,
        _type_id: i32,
    ) -> Result<Vec<EsiMarketHistoryDay>, EsiError> {
        Ok(Vec::new())
    }
}
```

- [ ] **Step 6: Run parsing tests**

Run:

```bash
cargo test -p evetools-esi --test model_parsing
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/esi/Cargo.toml crates/esi/src crates/esi/tests
git commit -m "feat: add esi response models"
```

## Task 2: Implement Public ESI HTTP Client

**Files:**

- Modify: `crates/esi/src/client.rs`
- Modify: `crates/esi/src/lib.rs`
- Create: `crates/esi/tests/client.rs`

- [ ] **Step 1: Write HTTP client tests**

Create `crates/esi/tests/client.rs`:

```rust
use evetools_esi::{EsiClient, EsiError, EsiOrderType};
use httpmock::prelude::*;

#[tokio::test]
async fn resolves_inventory_type_by_name() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/latest/universe/ids/")
            .query_param("datasource", "tranquility")
            .json_body(vec!["Tritanium"]);
        then.status(200)
            .header("content-type", "application/json")
            .body(include_str!("fixtures/universe_ids.json"));
    });

    let client = EsiClient::new(server.base_url());
    let resolved = client.resolve_inventory_type("Tritanium").await.unwrap();

    mock.assert();
    assert_eq!(resolved.type_id, 34);
    assert_eq!(resolved.name, "Tritanium");
}

#[tokio::test]
async fn resolves_inventory_type_by_numeric_id() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/latest/universe/types/34/")
            .query_param("datasource", "tranquility");
        then.status(200)
            .header("content-type", "application/json")
            .body(include_str!("fixtures/type_info.json"));
    });

    let client = EsiClient::new(server.base_url());
    let resolved = client.resolve_inventory_type("34").await.unwrap();

    mock.assert();
    assert_eq!(resolved.type_id, 34);
    assert_eq!(resolved.name, "Tritanium");
}

#[tokio::test]
async fn fetches_all_market_order_pages() {
    let server = MockServer::start();
    let page_one = server.mock(|when, then| {
        when.method(GET)
            .path("/latest/markets/10000002/orders/")
            .query_param("datasource", "tranquility")
            .query_param("order_type", "all")
            .query_param("type_id", "34")
            .query_param("page", "1");
        then.status(200)
            .header("content-type", "application/json")
            .header("X-Pages", "2")
            .body(include_str!("fixtures/market_orders.json"));
    });
    let page_two = server.mock(|when, then| {
        when.method(GET)
            .path("/latest/markets/10000002/orders/")
            .query_param("datasource", "tranquility")
            .query_param("order_type", "all")
            .query_param("type_id", "34")
            .query_param("page", "2");
        then.status(200)
            .header("content-type", "application/json")
            .header("X-Pages", "2")
            .body("[]");
    });

    let client = EsiClient::new(server.base_url());
    let orders = client
        .market_orders(10000002, 34, EsiOrderType::All)
        .await
        .unwrap();

    page_one.assert();
    page_two.assert();
    assert_eq!(orders.len(), 2);
    assert_eq!(orders[0].order_id, 7000000001);
}

#[tokio::test]
async fn fetches_market_history() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/latest/markets/10000002/history/")
            .query_param("datasource", "tranquility")
            .query_param("type_id", "34");
        then.status(200)
            .header("content-type", "application/json")
            .body(include_str!("fixtures/market_history.json"));
    });

    let client = EsiClient::new(server.base_url());
    let history = client.market_history(10000002, 34).await.unwrap();

    mock.assert();
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].volume, 1_250_000);
}

#[tokio::test]
async fn maps_not_found_status_to_item_not_found() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/latest/universe/types/999999/")
            .query_param("datasource", "tranquility");
        then.status(404)
            .header("content-type", "application/json")
            .body("{\"error\":\"not found\"}");
    });

    let client = EsiClient::new(server.base_url());
    let error = client.resolve_inventory_type("999999").await.unwrap_err();

    assert!(matches!(error, EsiError::ItemNotFound));
}
```

- [ ] **Step 2: Run HTTP client tests and verify they fail against the skeleton**

Run:

```bash
cargo test -p evetools-esi --test client
```

Expected: FAIL because `universe_ids`, `type_info`, `market_orders`, and `market_history` return skeleton values.

- [ ] **Step 3: Implement request helpers and public methods**

Replace `crates/esi/src/client.rs`:

```rust
use crate::{
    EsiError, EsiMarketHistoryDay, EsiMarketOrder, EsiOrderType, EsiTypeInfo,
    ResolvedInventoryType, UniverseIdsResponse,
};

#[derive(Clone, Debug)]
pub struct EsiClient {
    base_url: String,
    http: reqwest::Client,
}

impl EsiClient {
    pub fn tranquility() -> Self {
        Self::new("https://esi.evetech.net")
    }

    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn resolve_inventory_type(
        &self,
        query: &str,
    ) -> Result<ResolvedInventoryType, EsiError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Err(EsiError::ItemNotFound);
        }

        if let Ok(type_id) = trimmed.parse::<i32>() {
            let info = self.type_info(type_id).await?;
            return Ok(ResolvedInventoryType {
                type_id: info.type_id,
                name: info.name,
            });
        }

        let ids = self.universe_ids(trimmed).await?;
        let entry = ids
            .inventory_types
            .unwrap_or_default()
            .into_iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(trimmed))
            .ok_or(EsiError::ItemNotFound)?;

        Ok(ResolvedInventoryType {
            type_id: entry.id,
            name: entry.name,
        })
    }

    pub async fn universe_ids(&self, name: &str) -> Result<UniverseIdsResponse, EsiError> {
        let url = format!("{}/latest/universe/ids/", self.base_url);
        let response = self
            .http
            .post(url)
            .query(&[("datasource", "tranquility")])
            .json(&vec![name])
            .send()
            .await?;

        self.decode_response(response).await
    }

    pub async fn type_info(&self, type_id: i32) -> Result<EsiTypeInfo, EsiError> {
        let url = format!("{}/latest/universe/types/{}/", self.base_url, type_id);
        let response = self
            .http
            .get(url)
            .query(&[("datasource", "tranquility")])
            .send()
            .await?;

        self.decode_response(response).await
    }

    pub async fn market_orders(
        &self,
        region_id: i32,
        type_id: i32,
        order_type: EsiOrderType,
    ) -> Result<Vec<EsiMarketOrder>, EsiError> {
        let mut page = 1;
        let mut total_pages = 1;
        let mut orders = Vec::new();

        while page <= total_pages {
            let url = format!("{}/latest/markets/{}/orders/", self.base_url, region_id);
            let response = self
                .http
                .get(url)
                .query(&[
                    ("datasource", "tranquility".to_string()),
                    ("order_type", order_type.as_query_value().to_string()),
                    ("type_id", type_id.to_string()),
                    ("page", page.to_string()),
                ])
                .send()
                .await?;

            total_pages = response
                .headers()
                .get("X-Pages")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<i32>().ok())
                .unwrap_or(1);

            let mut page_orders: Vec<EsiMarketOrder> = self.decode_response(response).await?;
            orders.append(&mut page_orders);
            page += 1;
        }

        Ok(orders)
    }

    pub async fn market_history(
        &self,
        region_id: i32,
        type_id: i32,
    ) -> Result<Vec<EsiMarketHistoryDay>, EsiError> {
        let url = format!("{}/latest/markets/{}/history/", self.base_url, region_id);
        let response = self
            .http
            .get(url)
            .query(&[
                ("datasource", "tranquility".to_string()),
                ("type_id", type_id.to_string()),
            ])
            .send()
            .await?;

        self.decode_response(response).await
    }

    async fn decode_response<T>(&self, response: reqwest::Response) -> Result<T, EsiError>
    where
        T: serde::de::DeserializeOwned,
    {
        let status = response.status();
        let body = response.text().await?;

        if status.as_u16() == 404 {
            return Err(EsiError::ItemNotFound);
        }

        if !status.is_success() {
            return Err(EsiError::Status {
                status: status.as_u16(),
                body,
            });
        }

        serde_json::from_str(&body).map_err(EsiError::Decode)
    }
}
```

- [ ] **Step 4: Make `EsiError` comparable for selected tests**

Modify `crates/esi/src/lib.rs` so `EsiError` keeps `matches!` support without deriving `PartialEq` over foreign error types. The enum from Task 1 already supports the `matches!` assertion used in tests; no derive is needed.

- [ ] **Step 5: Run ESI tests**

Run:

```bash
cargo test -p evetools-esi
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/esi
git commit -m "feat: add public esi client"
```

## Task 3: Add Deterministic Domain Market Analysis

**Files:**

- Create: `crates/domain/src/market_analysis.rs`
- Modify: `crates/domain/src/lib.rs`
- Modify: `crates/domain/src/views.rs`

- [ ] **Step 1: Write market analysis tests**

Create `crates/domain/src/market_analysis.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::JITA_4_4_STATION_ID;
    use rust_decimal::Decimal;

    fn order(
        is_buy_order: bool,
        price: Decimal,
        volume_remain: u64,
        location_id: i64,
    ) -> PublicMarketOrder {
        PublicMarketOrder {
            type_id: 34,
            location_id,
            is_buy_order,
            price,
            volume_remain,
        }
    }

    #[test]
    fn summarizes_jita_top_of_book_and_ignores_other_locations() {
        let orders = vec![
            order(true, Decimal::new(501, 2), 500_000, JITA_4_4_STATION_ID),
            order(true, Decimal::new(501, 2), 125_000, JITA_4_4_STATION_ID),
            order(true, Decimal::new(502, 2), 999_000, 60008494),
            order(false, Decimal::new(549, 2), 620_000, JITA_4_4_STATION_ID),
            order(false, Decimal::new(549, 2), 30_000, JITA_4_4_STATION_ID),
            order(false, Decimal::new(548, 2), 900_000, 60008494),
        ];
        let history = vec![
            PublicMarketHistoryDay {
                average: Decimal::new(510, 2),
                date: "2026-05-24".to_string(),
                volume: 1_000_000,
            },
            PublicMarketHistoryDay {
                average: Decimal::new(518, 2),
                date: "2026-05-25".to_string(),
                volume: 1_250_000,
            },
        ];

        let summary = summarize_jita_market(
            34,
            "Tritanium",
            &orders,
            &history,
            "2026-05-25T12:00:00Z",
        );

        assert_eq!(summary.type_id, 34);
        assert_eq!(summary.item_name, "Tritanium");
        assert_eq!(summary.best_bid, Decimal::new(501, 2));
        assert_eq!(summary.best_ask, Decimal::new(549, 2));
        assert_eq!(summary.top_buy_depth, 625_000);
        assert_eq!(summary.top_sell_depth, 650_000);
        assert_eq!(summary.daily_volume, 1_250_000);
        assert_eq!(classify_price_trend(&history), PriceTrend::Up);
    }

    #[test]
    fn marks_missing_when_jita_lacks_one_side() {
        let orders = vec![order(true, Decimal::new(501, 2), 10, JITA_4_4_STATION_ID)];
        let summary = summarize_jita_market(
            34,
            "Tritanium",
            &orders,
            &[],
            "2026-05-25T12:00:00Z",
        );

        assert_eq!(summary.best_bid, Decimal::new(501, 2));
        assert_eq!(summary.best_ask, Decimal::ZERO);
        assert_eq!(summary.data_quality(), crate::DataQuality::Missing);
    }

    #[test]
    fn price_trend_uses_one_percent_threshold() {
        let stable = vec![
            PublicMarketHistoryDay {
                average: Decimal::new(10000, 2),
                date: "2026-05-24".to_string(),
                volume: 100,
            },
            PublicMarketHistoryDay {
                average: Decimal::new(10050, 2),
                date: "2026-05-25".to_string(),
                volume: 100,
            },
        ];
        let down = vec![
            PublicMarketHistoryDay {
                average: Decimal::new(10000, 2),
                date: "2026-05-24".to_string(),
                volume: 100,
            },
            PublicMarketHistoryDay {
                average: Decimal::new(9800, 2),
                date: "2026-05-25".to_string(),
                volume: 100,
            },
        ];

        assert_eq!(classify_price_trend(&stable), PriceTrend::Stable);
        assert_eq!(classify_price_trend(&down), PriceTrend::Down);
        assert_eq!(classify_price_trend(&[]), PriceTrend::Stable);
    }
}
```

- [ ] **Step 2: Run domain analysis tests and verify they fail before types exist**

Run:

```bash
cargo test -p evetools-domain market_analysis
```

Expected: FAIL with unresolved types and functions such as `PublicMarketOrder`, `summarize_jita_market`, and `PriceTrend`.

- [ ] **Step 3: Implement market analysis types and functions above the tests**

Add this implementation at the top of `crates/domain/src/market_analysis.rs`, above the test module:

```rust
use crate::{JITA_4_4_STATION_ID, OrderBookSummary};
use rust_decimal::Decimal;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicMarketOrder {
    pub type_id: i32,
    pub location_id: i64,
    pub is_buy_order: bool,
    pub price: Decimal,
    pub volume_remain: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicMarketHistoryDay {
    pub average: Decimal,
    pub date: String,
    pub volume: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PriceTrend {
    Up,
    Down,
    Stable,
}

impl PriceTrend {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Stable => "stable",
        }
    }
}

pub fn summarize_jita_market(
    type_id: i32,
    item_name: impl Into<String>,
    orders: &[PublicMarketOrder],
    history: &[PublicMarketHistoryDay],
    last_synced_at: impl Into<String>,
) -> OrderBookSummary {
    let jita_orders: Vec<&PublicMarketOrder> = orders
        .iter()
        .filter(|order| order.location_id == JITA_4_4_STATION_ID)
        .collect();

    let best_bid = jita_orders
        .iter()
        .filter(|order| order.is_buy_order)
        .map(|order| order.price)
        .max()
        .unwrap_or(Decimal::ZERO);

    let best_ask = jita_orders
        .iter()
        .filter(|order| !order.is_buy_order)
        .map(|order| order.price)
        .min()
        .unwrap_or(Decimal::ZERO);

    let top_buy_depth = jita_orders
        .iter()
        .filter(|order| order.is_buy_order && order.price == best_bid)
        .map(|order| order.volume_remain)
        .sum();

    let top_sell_depth = jita_orders
        .iter()
        .filter(|order| !order.is_buy_order && order.price == best_ask)
        .map(|order| order.volume_remain)
        .sum();

    let daily_volume = history.last().map(|day| day.volume).unwrap_or(0);

    OrderBookSummary {
        type_id,
        item_name: item_name.into(),
        best_bid,
        best_ask,
        daily_volume,
        top_buy_depth,
        top_sell_depth,
        last_synced_at: last_synced_at.into(),
    }
}

pub fn classify_price_trend(history: &[PublicMarketHistoryDay]) -> PriceTrend {
    if history.len() < 2 {
        return PriceTrend::Stable;
    }

    let previous = history[history.len() - 2].average;
    let latest = history[history.len() - 1].average;

    if previous <= Decimal::ZERO {
        return PriceTrend::Stable;
    }

    let pct_change = ((latest - previous) / previous) * Decimal::from(100);
    if pct_change > Decimal::ONE {
        PriceTrend::Up
    } else if pct_change < -Decimal::ONE {
        PriceTrend::Down
    } else {
        PriceTrend::Stable
    }
}
```

- [ ] **Step 4: Export market analysis**

Modify `crates/domain/src/lib.rs`:

```rust
pub mod fixtures;
pub mod market;
pub mod market_analysis;
pub mod scoring;
pub mod views;

pub use market::{DataQuality, JITA_4_4_STATION_ID, OrderBookSummary, THE_FORGE_REGION_ID};
pub use market_analysis::{
    classify_price_trend, summarize_jita_market, PriceTrend, PublicMarketHistoryDay,
    PublicMarketOrder,
};
pub use scoring::{attention_score, gross_spread, liquidity_score, net_profit, FeeProfile};
pub use views::{MarketLookupView, OrderMonitorView, SelectionCandidateView};
```

- [ ] **Step 5: Add `MarketLookupView` conversion tests**

Append tests to `crates/domain/src/views.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OrderBookSummary, PriceTrend};
    use rust_decimal::Decimal;

    #[test]
    fn market_lookup_view_formats_summary_values() {
        let summary = OrderBookSummary {
            type_id: 34,
            item_name: "Tritanium".to_string(),
            best_bid: Decimal::new(501, 2),
            best_ask: Decimal::new(549, 2),
            daily_volume: 1_250_000,
            top_buy_depth: 625_000,
            top_sell_depth: 650_000,
            last_synced_at: "2026-05-25T12:00:00Z".to_string(),
        };

        let view = MarketLookupView::from_summary(summary, PriceTrend::Up);

        assert_eq!(view.best_bid, "5.01");
        assert_eq!(view.best_ask, "5.49");
        assert_eq!(view.spread, "0.48");
        assert_eq!(view.spread_percent, "9.58");
        assert_eq!(view.price_trend, "up");
        assert_eq!(view.data_quality, "fresh");
    }
}
```

- [ ] **Step 6: Run view test and verify it fails before constructor exists**

Run:

```bash
cargo test -p evetools-domain market_lookup_view_formats_summary_values
```

Expected: FAIL because `MarketLookupView::from_summary` is not defined.

- [ ] **Step 7: Implement `MarketLookupView::from_summary`**

Add to `crates/domain/src/views.rs` below the struct definitions:

```rust
use crate::{DataQuality, OrderBookSummary, PriceTrend};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

fn format_isk(value: Decimal) -> String {
    format!("{:.2}", value.round_dp(2).to_f64().unwrap_or(0.0))
}

fn data_quality_code(value: DataQuality) -> &'static str {
    match value {
        DataQuality::Fresh => "fresh",
        DataQuality::Stale => "stale",
        DataQuality::Sparse => "sparse",
        DataQuality::Missing => "missing",
    }
}

impl MarketLookupView {
    pub fn from_summary(summary: OrderBookSummary, trend: PriceTrend) -> Self {
        Self {
            type_id: summary.type_id,
            item_name: summary.item_name.clone(),
            best_bid: format_isk(summary.best_bid),
            best_ask: format_isk(summary.best_ask),
            spread: format_isk(summary.spread()),
            spread_percent: format_isk(summary.spread_percent()),
            daily_volume: summary.daily_volume,
            price_trend: trend.as_code().to_string(),
            top_buy_depth: summary.top_buy_depth,
            top_sell_depth: summary.top_sell_depth,
            last_synced_at: summary.last_synced_at.clone(),
            data_quality: data_quality_code(summary.data_quality()).to_string(),
        }
    }
}
```

Keep existing struct definitions in the same file. Place the `use` lines at the top of the file so imports compile cleanly.

- [ ] **Step 8: Run domain tests**

Run:

```bash
cargo test -p evetools-domain
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/domain/src/lib.rs crates/domain/src/market_analysis.rs crates/domain/src/views.rs
git commit -m "feat: summarize public jita market data"
```

## Task 4: Add Selection Candidate Builder

**Files:**

- Modify: `crates/domain/src/market_analysis.rs`
- Modify: `crates/domain/src/views.rs`

- [ ] **Step 1: Write candidate builder tests**

Append to the existing test module in `crates/domain/src/market_analysis.rs`:

```rust
#[test]
fn builds_selection_candidate_from_summary_and_fee_profile() {
    let summary = OrderBookSummary {
        type_id: 34,
        item_name: "Tritanium".to_string(),
        best_bid: Decimal::new(501, 2),
        best_ask: Decimal::new(549, 2),
        daily_volume: 1_250_000,
        top_buy_depth: 625_000,
        top_sell_depth: 650_000,
        last_synced_at: "2026-05-25T12:00:00Z".to_string(),
    };

    let candidate = build_selection_candidate(&summary, &FeeProfile::conservative_default());

    assert_eq!(candidate.type_id, 34);
    assert_eq!(candidate.item_name, "Tritanium");
    assert_eq!(candidate.recommended_entry_price, Decimal::new(502, 2));
    assert_eq!(candidate.recommended_exit_price, Decimal::new(548, 2));
    assert!(candidate.net_profit > Decimal::ZERO);
    assert!(candidate.attention_score >= 80);
    assert!(candidate.reason_codes.contains(&"healthy_spread".to_string()));
    assert!(candidate.reason_codes.contains(&"high_daily_volume".to_string()));
    assert!(candidate.reason_codes.contains(&"deep_top_book".to_string()));
}

#[test]
fn candidate_reasons_explain_sparse_or_missing_data() {
    let summary = OrderBookSummary {
        type_id: 999,
        item_name: "Slow Item".to_string(),
        best_bid: Decimal::new(100, 2),
        best_ask: Decimal::new(101, 2),
        daily_volume: 3,
        top_buy_depth: 1,
        top_sell_depth: 1,
        last_synced_at: "2026-05-25T12:00:00Z".to_string(),
    };

    let candidate = build_selection_candidate(&summary, &FeeProfile::conservative_default());

    assert!(candidate.reason_codes.contains(&"sparse_market_data".to_string()));
    assert!(candidate.attention_score < 40);
}
```

- [ ] **Step 2: Run candidate tests and verify they fail before builder exists**

Run:

```bash
cargo test -p evetools-domain candidate
```

Expected: FAIL because `build_selection_candidate` and `CandidateAnalysis` are missing.

- [ ] **Step 3: Implement candidate analysis builder**

Add to `crates/domain/src/market_analysis.rs` below `classify_price_trend`:

```rust
use crate::{attention_score, liquidity_score, net_profit, DataQuality, FeeProfile};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateAnalysis {
    pub type_id: i32,
    pub item_name: String,
    pub recommended_entry_price: Decimal,
    pub recommended_exit_price: Decimal,
    pub net_profit: Decimal,
    pub attention_score: u8,
    pub liquidity_score: u8,
    pub confidence_score: u8,
    pub reason_codes: Vec<String>,
}

pub fn build_selection_candidate(
    summary: &OrderBookSummary,
    fee: &FeeProfile,
) -> CandidateAnalysis {
    let recommended_entry_price = if summary.best_bid > Decimal::ZERO {
        summary.best_bid + Decimal::new(1, 2)
    } else {
        Decimal::ZERO
    };
    let recommended_exit_price = if summary.best_ask > Decimal::new(1, 2) {
        summary.best_ask - Decimal::new(1, 2)
    } else {
        Decimal::ZERO
    };
    let estimated_net_profit = if recommended_entry_price > Decimal::ZERO
        && recommended_exit_price > Decimal::ZERO
    {
        net_profit(recommended_entry_price, recommended_exit_price, fee)
    } else {
        Decimal::ZERO
    };
    let net_margin_pct = if recommended_entry_price > Decimal::ZERO {
        (estimated_net_profit / recommended_entry_price) * Decimal::from(100)
    } else {
        Decimal::ZERO
    };
    let top_depth = summary.top_buy_depth.min(summary.top_sell_depth);
    let liquidity = liquidity_score(summary.daily_volume, top_depth);
    let attention = attention_score(net_margin_pct, summary.daily_volume, top_depth);
    let confidence = confidence_score(summary.data_quality(), liquidity, estimated_net_profit);
    let reason_codes = candidate_reason_codes(summary, estimated_net_profit);

    CandidateAnalysis {
        type_id: summary.type_id,
        item_name: summary.item_name.clone(),
        recommended_entry_price,
        recommended_exit_price,
        net_profit: estimated_net_profit,
        attention_score: attention,
        liquidity_score: liquidity,
        confidence_score: confidence,
        reason_codes,
    }
}

fn confidence_score(data_quality: DataQuality, liquidity: u8, net_profit_value: Decimal) -> u8 {
    let quality_score = match data_quality {
        DataQuality::Fresh => 100,
        DataQuality::Sparse => 45,
        DataQuality::Missing => 0,
        DataQuality::Stale => 35,
    };
    let profit_score = if net_profit_value > Decimal::ZERO { 100 } else { 20 };

    ((quality_score as u16 * 50 + liquidity as u16 * 30 + profit_score as u16 * 20) / 100) as u8
}

fn candidate_reason_codes(summary: &OrderBookSummary, net_profit_value: Decimal) -> Vec<String> {
    let mut reasons = Vec::new();

    match summary.data_quality() {
        DataQuality::Fresh => {}
        DataQuality::Sparse => reasons.push("sparse_market_data".to_string()),
        DataQuality::Missing => reasons.push("missing_market_side".to_string()),
        DataQuality::Stale => reasons.push("stale_market_data".to_string()),
    }

    if summary.spread_percent() >= Decimal::new(5, 0) && net_profit_value > Decimal::ZERO {
        reasons.push("healthy_spread".to_string());
    } else if summary.spread_percent() >= Decimal::new(2, 0) {
        reasons.push("acceptable_spread".to_string());
    }

    if summary.daily_volume >= 1_000_000 {
        reasons.push("high_daily_volume".to_string());
    } else if summary.daily_volume >= 1_000 {
        reasons.push("moderate_velocity".to_string());
    }

    if summary.top_buy_depth.min(summary.top_sell_depth) >= 100_000 {
        reasons.push("deep_top_book".to_string());
    }

    if net_profit_value <= Decimal::ZERO {
        reasons.push("negative_net_profit".to_string());
    }

    reasons
}
```

Adjust the top of `crates/domain/src/market_analysis.rs` so all imports are consolidated:

```rust
use crate::{
    attention_score, liquidity_score, net_profit, DataQuality, FeeProfile, JITA_4_4_STATION_ID,
    OrderBookSummary,
};
use rust_decimal::Decimal;
```

- [ ] **Step 4: Export candidate analysis**

Modify `crates/domain/src/lib.rs` export block:

```rust
pub use market_analysis::{
    build_selection_candidate, classify_price_trend, summarize_jita_market, CandidateAnalysis,
    PriceTrend, PublicMarketHistoryDay, PublicMarketOrder,
};
```

- [ ] **Step 5: Add `SelectionCandidateView` conversion test**

Append to the existing test module in `crates/domain/src/views.rs`:

```rust
#[test]
fn selection_candidate_view_formats_analysis_values() {
    let analysis = crate::CandidateAnalysis {
        type_id: 34,
        item_name: "Tritanium".to_string(),
        recommended_entry_price: Decimal::new(502, 2),
        recommended_exit_price: Decimal::new(548, 2),
        net_profit: Decimal::new(20, 2),
        attention_score: 82,
        liquidity_score: 96,
        confidence_score: 88,
        reason_codes: vec!["healthy_spread".to_string()],
    };

    let view = SelectionCandidateView::from_analysis(analysis);

    assert_eq!(view.recommended_entry_price, "5.02");
    assert_eq!(view.recommended_exit_price, "5.48");
    assert_eq!(view.net_profit, "0.20");
    assert_eq!(view.reason_codes, vec!["healthy_spread"]);
}
```

- [ ] **Step 6: Run view test and verify it fails before constructor exists**

Run:

```bash
cargo test -p evetools-domain selection_candidate_view_formats_analysis_values
```

Expected: FAIL because `SelectionCandidateView::from_analysis` is not defined.

- [ ] **Step 7: Implement `SelectionCandidateView::from_analysis`**

Add this implementation below `impl MarketLookupView` in `crates/domain/src/views.rs`:

```rust
impl SelectionCandidateView {
    pub fn from_analysis(analysis: crate::CandidateAnalysis) -> Self {
        Self {
            type_id: analysis.type_id,
            item_name: analysis.item_name,
            recommended_entry_price: format_isk(analysis.recommended_entry_price),
            recommended_exit_price: format_isk(analysis.recommended_exit_price),
            net_profit: format_isk(analysis.net_profit),
            attention_score: analysis.attention_score,
            liquidity_score: analysis.liquidity_score,
            confidence_score: analysis.confidence_score,
            reason_codes: analysis.reason_codes,
        }
    }
}
```

- [ ] **Step 8: Run domain tests**

Run:

```bash
cargo test -p evetools-domain
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/domain/src/lib.rs crates/domain/src/market_analysis.rs crates/domain/src/views.rs
git commit -m "feat: build public selection candidates"
```

## Task 5: Wire Live Public Market Service into Tauri Commands

**Files:**

- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `crates/worker/src/lib.rs`

- [ ] **Step 1: Write command behavior tests**

Replace the existing test module in `apps/desktop/src-tauri/src/lib.rs` with async tests that target helper functions:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lookup_rejects_empty_query() {
        let result = lookup_market_price_with_source("   ".to_string(), MarketSource::Fixture).await;
        assert_eq!(result.unwrap_err(), "Item query is required");
    }

    #[tokio::test]
    async fn fixture_source_returns_mvp_views_without_network() {
        assert_eq!(
            lookup_market_price_with_source("Tritanium".to_string(), MarketSource::Fixture)
                .await
                .unwrap()
                .item_name,
            "Tritanium"
        );
        assert_eq!(
            list_selection_candidates_with_source(MarketSource::Fixture)
                .await
                .unwrap()
                .len(),
            2
        );
        assert_eq!(list_order_monitor_items().unwrap().len(), 2);
    }

    #[test]
    fn worker_status_reports_live_fixture_and_fallback_sources() {
        assert_eq!(evetools_worker::live_sync_status().public_market_sync, "live-ready");
        assert_eq!(evetools_worker::live_sync_status().data_source, "live");
        assert_eq!(
            evetools_worker::fixture_fallback_sync_status().public_market_sync,
            "fixture-fallback"
        );
        assert_eq!(
            evetools_worker::fixture_fallback_sync_status().data_source,
            "fixture"
        );
    }

    #[test]
    fn sync_status_uses_last_public_market_fallback_signal() {
        mark_public_market_fallback(false);
        assert_eq!(
            get_sync_status_with_source(MarketSource::Fixture)
                .unwrap()
                .public_market_sync,
            "fixture-ready"
        );
        assert_eq!(
            get_sync_status_with_source(MarketSource::Live(EsiClient::new("http://127.0.0.1:9")))
                .unwrap()
                .public_market_sync,
            "live-ready"
        );

        mark_public_market_fallback(true);
        assert_eq!(
            get_sync_status_with_source(MarketSource::Live(EsiClient::new("http://127.0.0.1:9")))
                .unwrap()
                .public_market_sync,
            "fixture-fallback"
        );
        mark_public_market_fallback(false);
    }
}
```

- [ ] **Step 2: Run desktop command tests and verify they fail before helpers exist**

Run:

```bash
cargo test -p evetools-desktop
```

Expected: FAIL because `MarketSource`, `lookup_market_price_with_source`, `list_selection_candidates_with_source`, `get_sync_status_with_source`, `mark_public_market_fallback`, `live_sync_status`, `fixture_fallback_sync_status`, and `SyncStatus::data_source` are missing.

- [ ] **Step 3: Extend worker sync status**

Replace `crates/worker/src/lib.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncStatus {
    pub public_market_sync: String,
    pub authenticated_order_sync: String,
    pub data_source: String,
}

pub fn fixture_sync_status() -> SyncStatus {
    SyncStatus {
        public_market_sync: "fixture-ready".to_string(),
        authenticated_order_sync: "not-authorized".to_string(),
        data_source: "fixture".to_string(),
    }
}

pub fn live_sync_status() -> SyncStatus {
    SyncStatus {
        public_market_sync: "live-ready".to_string(),
        authenticated_order_sync: "not-authorized".to_string(),
        data_source: "live".to_string(),
    }
}

pub fn fixture_fallback_sync_status() -> SyncStatus {
    SyncStatus {
        public_market_sync: "fixture-fallback".to_string(),
        authenticated_order_sync: "not-authorized".to_string(),
        data_source: "fixture".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_reports_separate_public_private_and_source_status() {
        let fixture = fixture_sync_status();
        assert_eq!(fixture.public_market_sync, "fixture-ready");
        assert_eq!(fixture.authenticated_order_sync, "not-authorized");
        assert_eq!(fixture.data_source, "fixture");

        let live = live_sync_status();
        assert_eq!(live.public_market_sync, "live-ready");
        assert_eq!(live.data_source, "live");

        let fallback = fixture_fallback_sync_status();
        assert_eq!(fallback.public_market_sync, "fixture-fallback");
        assert_eq!(fallback.data_source, "fixture");
    }
}
```

- [ ] **Step 4: Add desktop crate dependencies**

Modify `apps/desktop/src-tauri/Cargo.toml` dependencies:

```toml
[dependencies]
chrono.workspace = true
evetools-domain = { path = "../../../crates/domain" }
evetools-esi = { path = "../../../crates/esi" }
evetools-worker = { path = "../../../crates/worker" }
rust_decimal.workspace = true
serde.workspace = true
tauri.workspace = true
tokio.workspace = true
```

Keep existing `build-dependencies` unchanged.

- [ ] **Step 5: Implement live-or-fixture command adapter**

Replace `apps/desktop/src-tauri/src/lib.rs`:

```rust
use chrono::Utc;
use evetools_domain::fixtures::{
    fixture_market_lookup, fixture_order_monitor, fixture_selection_candidates,
};
use evetools_domain::{
    build_selection_candidate, classify_price_trend, summarize_jita_market, FeeProfile,
    MarketLookupView, OrderMonitorView, PublicMarketHistoryDay, PublicMarketOrder,
    SelectionCandidateView, THE_FORGE_REGION_ID,
};
use evetools_esi::{EsiClient, EsiError, EsiMarketHistoryDay, EsiMarketOrder, EsiOrderType};
use evetools_worker::{
    fixture_fallback_sync_status, fixture_sync_status, live_sync_status, SyncStatus,
};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use std::sync::atomic::{AtomicBool, Ordering};

const SELECTION_SEED_TYPES: &[(i32, &str)] = &[
    (34, "Tritanium"),
    (35, "Pyerite"),
    (36, "Mexallon"),
    (37, "Isogen"),
];

static PUBLIC_MARKET_USED_FALLBACK: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug)]
enum MarketSource {
    Fixture,
    Live(EsiClient),
}

impl MarketSource {
    fn from_env() -> Self {
        match std::env::var("EVETOOLS_MARKET_SOURCE") {
            Ok(value) if value.eq_ignore_ascii_case("fixture") => Self::Fixture,
            _ => Self::Live(EsiClient::tranquility()),
        }
    }

    fn is_fixture(&self) -> bool {
        matches!(self, Self::Fixture)
    }
}

fn mark_public_market_fallback(used_fallback: bool) {
    PUBLIC_MARKET_USED_FALLBACK.store(used_fallback, Ordering::Relaxed);
}

fn public_market_used_fallback() -> bool {
    PUBLIC_MARKET_USED_FALLBACK.load(Ordering::Relaxed)
}

#[tauri::command]
async fn lookup_market_price(query: String) -> Result<MarketLookupView, String> {
    lookup_market_price_with_source(query, MarketSource::from_env()).await
}

async fn lookup_market_price_with_source(
    query: String,
    source: MarketSource,
) -> Result<MarketLookupView, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err("Item query is required".to_string());
    }

    match source {
        MarketSource::Fixture => {
            mark_public_market_fallback(false);
            Ok(fixture_market_lookup(trimmed))
        }
        MarketSource::Live(client) => match lookup_market_price_live(trimmed, &client).await {
            Ok(view) => {
                mark_public_market_fallback(false);
                Ok(view)
            }
            Err(EsiError::ItemNotFound) => {
                mark_public_market_fallback(false);
                Err("Item not found".to_string())
            }
            Err(_) => {
                mark_public_market_fallback(true);
                Ok(fixture_market_lookup(trimmed))
            }
        },
    }
}

async fn lookup_market_price_live(
    query: &str,
    client: &EsiClient,
) -> Result<MarketLookupView, EsiError> {
    let resolved = client.resolve_inventory_type(query).await?;
    let orders = client
        .market_orders(THE_FORGE_REGION_ID, resolved.type_id, EsiOrderType::All)
        .await?;
    let history = client
        .market_history(THE_FORGE_REGION_ID, resolved.type_id)
        .await?;

    let domain_orders = to_domain_orders(&orders);
    let domain_history = to_domain_history(&history);
    let summary = summarize_jita_market(
        resolved.type_id,
        resolved.name,
        &domain_orders,
        &domain_history,
        Utc::now().to_rfc3339(),
    );
    let trend = classify_price_trend(&domain_history);

    Ok(MarketLookupView::from_summary(summary, trend))
}

#[tauri::command]
async fn list_selection_candidates() -> Result<Vec<SelectionCandidateView>, String> {
    list_selection_candidates_with_source(MarketSource::from_env()).await
}

async fn list_selection_candidates_with_source(
    source: MarketSource,
) -> Result<Vec<SelectionCandidateView>, String> {
    match source {
        MarketSource::Fixture => {
            mark_public_market_fallback(false);
            Ok(fixture_selection_candidates())
        }
        MarketSource::Live(client) => {
            let mut candidates = Vec::new();
            for (type_id, item_name) in SELECTION_SEED_TYPES {
                if let Ok(candidate) = selection_candidate_live(*type_id, item_name, &client).await {
                    candidates.push(candidate);
                }
            }

            if candidates.is_empty() {
                mark_public_market_fallback(true);
                Ok(fixture_selection_candidates())
            } else {
                mark_public_market_fallback(false);
                candidates.sort_by(|left, right| {
                    right
                        .attention_score
                        .cmp(&left.attention_score)
                        .then_with(|| left.item_name.cmp(&right.item_name))
                });
                Ok(candidates)
            }
        }
    }
}

async fn selection_candidate_live(
    type_id: i32,
    item_name: &str,
    client: &EsiClient,
) -> Result<SelectionCandidateView, EsiError> {
    let orders = client
        .market_orders(THE_FORGE_REGION_ID, type_id, EsiOrderType::All)
        .await?;
    let history = client.market_history(THE_FORGE_REGION_ID, type_id).await?;
    let domain_orders = to_domain_orders(&orders);
    let domain_history = to_domain_history(&history);
    let summary = summarize_jita_market(
        type_id,
        item_name,
        &domain_orders,
        &domain_history,
        Utc::now().to_rfc3339(),
    );
    let analysis = build_selection_candidate(&summary, &FeeProfile::conservative_default());

    Ok(SelectionCandidateView::from_analysis(analysis))
}

fn to_domain_orders(orders: &[EsiMarketOrder]) -> Vec<PublicMarketOrder> {
    orders
        .iter()
        .filter_map(|order| {
            Some(PublicMarketOrder {
                type_id: order.type_id,
                location_id: order.location_id,
                is_buy_order: order.is_buy_order,
                price: Decimal::from_f64(order.price)?,
                volume_remain: u64::try_from(order.volume_remain).ok()?,
            })
        })
        .collect()
}

fn to_domain_history(history: &[EsiMarketHistoryDay]) -> Vec<PublicMarketHistoryDay> {
    history
        .iter()
        .filter_map(|day| {
            Some(PublicMarketHistoryDay {
                average: Decimal::from_f64(day.average)?,
                date: day.date.clone(),
                volume: u64::try_from(day.volume).ok()?,
            })
        })
        .collect()
}

#[tauri::command]
fn list_order_monitor_items() -> Result<Vec<OrderMonitorView>, String> {
    Ok(fixture_order_monitor())
}

#[tauri::command]
fn get_sync_status() -> Result<SyncStatus, String> {
    get_sync_status_with_source(MarketSource::from_env())
}

fn get_sync_status_with_source(source: MarketSource) -> Result<SyncStatus, String> {
    if source.is_fixture() {
        Ok(fixture_sync_status())
    } else if public_market_used_fallback() {
        Ok(fixture_fallback_sync_status())
    } else {
        Ok(live_sync_status())
    }
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            lookup_market_price,
            list_selection_candidates,
            list_order_monitor_items,
            get_sync_status
        ])
        .run(tauri::generate_context!())
        .expect("failed to run EveTools desktop application");
}
```

- [ ] **Step 6: Run desktop command tests**

Run:

```bash
cargo test -p evetools-desktop
```

Expected: PASS. If the crate package name differs, run `cargo metadata --no-deps --format-version 1 | jq '.packages[] | select(.manifest_path | contains("apps/desktop/src-tauri")) | .name'` and use that package name.

- [ ] **Step 7: Run full Rust tests**

Run:

```bash
cargo test --workspace
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/src/lib.rs crates/worker/src/lib.rs
git commit -m "feat: wire public esi market commands"
```

## Task 6: Update Frontend Labels for Live Public Sync

**Files:**

- Modify: `apps/desktop/src/commands.ts`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/i18n/resources.ts`

- [ ] **Step 1: Update TypeScript command types**

Modify `apps/desktop/src/commands.ts`:

```ts
export type SyncStatus = {
  public_market_sync: string;
  authenticated_order_sync: string;
  data_source: string;
};
```

Keep all existing invoke functions unchanged.

- [ ] **Step 2: Replace hard-coded data source in UI**

Modify `refresh()` in `apps/desktop/src/App.tsx` so status is fetched after lookup and selection commands complete:

```tsx
async function refresh() {
  setLoadState("loading");
  setError(null);
  try {
    const [lookupResult, candidateResult, orderResult] = await Promise.all([
      lookupMarketPrice(query),
      listSelectionCandidates(),
      listOrderMonitorItems()
    ]);
    const statusResult = await getSyncStatus();
    setLookup(lookupResult);
    setCandidates(candidateResult);
    setOrders(orderResult);
    setSyncStatus(statusResult);
    setLoadState("ready");
  } catch (err) {
    setError(err instanceof Error ? err.message : String(err));
    setLoadState("error");
  }
}
```

Modify the status row in the same file:

```tsx
<section className="status-row">
  <StatusCard label={t("statusCards.publicMarketSync")} value={code("codes.syncStatus", syncStatus?.public_market_sync)} />
  <StatusCard label={t("statusCards.orderSync")} value={code("codes.syncStatus", syncStatus?.authenticated_order_sync)} />
  <StatusCard label={t("statusCards.dataSource")} value={code("codes.dataSource", syncStatus?.data_source)} />
</section>
```

- [ ] **Step 3: Add i18n resource codes**

Modify `apps/desktop/src/i18n/resources.ts` in both `zh-CN.translation.codes` and `en-US.translation.codes`.

For `zh-CN`:

```ts
syncStatus: {
  "fixture-ready": "测试数据就绪",
  "fixture-fallback": "测试数据回退",
  "live-ready": "实时 ESI 就绪",
  "not-authorized": "未授权",
  unknown: "未知"
},
dataSource: {
  fixture: "测试数据",
  live: "实时 ESI",
  unknown: "未知"
},
trend: {
  up: "上涨",
  down: "下跌",
  stable: "稳定",
  unknown: "未知"
},
reason: {
  healthy_spread: "价差健康",
  high_daily_volume: "日成交量高",
  deep_top_book: "盘口深度充足",
  acceptable_spread: "价差可接受",
  moderate_velocity: "成交速度适中",
  sparse_market_data: "市场数据稀疏",
  missing_market_side: "缺少单侧盘口",
  stale_market_data: "市场数据过期",
  negative_net_profit: "预估净收益为负",
  undercut_detected: "检测到被压价",
  high_velocity_item: "高流动性物品",
  overbid_detected: "检测到被超价"
}
```

For `en-US`:

```ts
syncStatus: {
  "fixture-ready": "Fixture ready",
  "fixture-fallback": "Fixture fallback",
  "live-ready": "Live ESI ready",
  "not-authorized": "Not authorized",
  unknown: "Unknown"
},
dataSource: {
  fixture: "Fixture",
  live: "Live ESI",
  unknown: "Unknown"
},
trend: {
  up: "Up",
  down: "Down",
  stable: "Stable",
  unknown: "Unknown"
},
reason: {
  healthy_spread: "Healthy spread",
  high_daily_volume: "High daily volume",
  deep_top_book: "Deep top book",
  acceptable_spread: "Acceptable spread",
  moderate_velocity: "Moderate velocity",
  sparse_market_data: "Sparse market data",
  missing_market_side: "Missing one side of book",
  stale_market_data: "Stale market data",
  negative_net_profit: "Estimated net profit is negative",
  undercut_detected: "Undercut detected",
  high_velocity_item: "High velocity item",
  overbid_detected: "Overbid detected"
}
```

Keep existing neighboring keys such as `dataQuality`, `side`, and `action`.

- [ ] **Step 4: Run frontend checks**

Run:

```bash
pnpm check
```

Expected: PASS.

- [ ] **Step 5: Run Rust tests after frontend type update**

Run:

```bash
cargo test --workspace
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src/commands.ts apps/desktop/src/App.tsx apps/desktop/src/i18n/resources.ts
git commit -m "feat: show live public sync status"
```

## Task 7: Document Public ESI Mode and Verify End to End

**Files:**

- Modify: `README.md`

- [ ] **Step 1: Update README with public ESI section**

Add this section to `README.md` under the development or architecture notes:

```markdown
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
```

- [ ] **Step 2: Run formatting**

Run:

```bash
cargo fmt --all
```

Expected: command exits successfully and only Rust formatting changes appear.

- [ ] **Step 3: Run full Rust verification**

Run:

```bash
cargo test --workspace
```

Expected: PASS.

- [ ] **Step 4: Run frontend verification**

Run:

```bash
pnpm check
```

Expected: PASS.

- [ ] **Step 5: Run fixture mode manually**

Run:

```bash
EVETOOLS_MARKET_SOURCE=fixture pnpm dev
```

Expected:

- The app starts.
- Status shows fixture source.
- Market lookup for `Tritanium` returns fixture values.
- Selection discovery shows fixture candidates.
- Order monitor remains fixture-backed.

Stop the dev server after verification.

- [ ] **Step 6: Run live mode manually**

Run:

```bash
EVETOOLS_MARKET_SOURCE=live pnpm dev
```

Expected:

- The app starts.
- Status shows live ESI source.
- Market lookup for `Tritanium` returns live values when ESI is available.
- Numeric lookup for `34` resolves to `Tritanium`.
- Selection discovery shows live seed-pool candidates when ESI is available.
- If ESI fails, market lookup falls back to fixture values except for a true item-not-found response.

Stop the dev server after verification.

- [ ] **Step 7: Inspect worktree**

Run:

```bash
git status --short
```

Expected: only intentional files are modified.

- [ ] **Step 8: Commit documentation and final verification changes**

```bash
git add README.md Cargo.lock
git add Cargo.toml crates apps
git commit -m "docs: document public esi market sync"
```

If `git status --short` shows no staged changes after previous task commits, skip this commit and record that README changes were already committed.

## Self-Review Checklist

- Spec coverage: This plan covers the public ESI price lookup and selection discovery items from the MVP spec. It intentionally leaves SSO, authenticated order sync, SQLite snapshots, scheduled background sync, watchlists, and fee-profile UI for later phases.
- Boundary check: HTTP and ESI response details stay in `crates/esi`; deterministic aggregation and scoring stay in `crates/domain`; Tauri commands only validate, convert, and orchestrate.
- Testability: ESI tests use `httpmock` and fixtures; domain tests use pure Rust inputs; desktop command tests run in fixture source mode.
- Runtime fallback: The plan keeps fixture mode available and adds fixture fallback for live ESI failures.
- Localization: New status, source, trend, and reason codes are added to both zh-CN and en-US resources.
- Precision: Rust decimal conversion happens before domain calculations; frontend receives formatted strings.
