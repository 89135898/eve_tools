# Tauri Desktop Foundation Implementation Plan

> Status update, 2026-05-29: this is a historical implementation plan for the first fixture-backed desktop slice. The current app has since added Supabase/Postgres catalog and market snapshots, hosted HTTP reads, public market sync, EVE SSO, authenticated character order sync, and real Order Monitor rows. Use `README.md` and the current architecture/design specs for the up-to-date runtime shape.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first runnable desktop slice: Tauri 2 shell, React UI, Rust workspace crates, tested domain calculations, and fixture-backed commands for price lookup, selection discovery, and order monitoring.

**Architecture:** The app is desktop-first. React/Vite renders the interface inside Tauri, while Rust owns command handlers and all business logic. This first slice uses fixture data through Tauri commands so the UI, command boundary, and domain scoring can be validated before adding real ESI sync and SSO.

**Tech Stack:** Rust, Cargo workspace, Tauri 2, Tokio, serde, rust_decimal, React, Vite, TypeScript, TanStack Table-ready structure, pnpm.

---

## Scope Check

The approved MVP includes three larger subsystems: desktop foundation, public ESI market sync, and SSO-based order monitoring. This plan implements the first subsystem only: the runnable desktop foundation with deterministic fixture data. Follow-up plans should add real public ESI sync, SQLite persistence, then EVE SSO and authenticated order sync.

## File Structure

Create these files:

- `Cargo.toml`: Rust workspace root.
- `package.json`: pnpm workspace scripts.
- `pnpm-workspace.yaml`: pnpm package discovery.
- `crates/domain/Cargo.toml`: domain crate dependencies.
- `crates/domain/src/lib.rs`: domain module exports.
- `crates/domain/src/market.rs`: market constants, price-state model, data-quality helpers.
- `crates/domain/src/scoring.rs`: spread, net-profit, liquidity, and urgency calculations.
- `crates/domain/src/views.rs`: serialized view models returned to the UI.
- `crates/domain/src/fixtures.rs`: deterministic fixture views used by Tauri commands.
- `crates/esi/Cargo.toml`, `crates/esi/src/lib.rs`: ESI crate shell with typed error boundary.
- `crates/db/Cargo.toml`, `crates/db/src/lib.rs`: DB crate shell with typed error boundary.
- `crates/worker/Cargo.toml`, `crates/worker/src/lib.rs`: worker crate shell with typed status boundary.
- `apps/desktop/package.json`: desktop package scripts and JS dependencies.
- `apps/desktop/index.html`: Vite entry document.
- `apps/desktop/tsconfig.json`: TypeScript config.
- `apps/desktop/vite.config.ts`: Vite config for Tauri.
- `apps/desktop/src/main.tsx`: React entrypoint.
- `apps/desktop/src/App.tsx`: desktop UI shell.
- `apps/desktop/src/commands.ts`: typed Tauri command wrappers.
- `apps/desktop/src/styles.css`: dashboard styling.
- `apps/desktop/src-tauri/Cargo.toml`: Tauri Rust crate.
- `apps/desktop/src-tauri/build.rs`: Tauri build script.
- `apps/desktop/src-tauri/tauri.conf.json`: Tauri app config.
- `apps/desktop/src-tauri/capabilities/default.json`: Tauri permission capability.
- `apps/desktop/src-tauri/src/main.rs`: native entrypoint.
- `apps/desktop/src-tauri/src/lib.rs`: Tauri command handlers.

## Task 1: Workspace Scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `package.json`
- Create: `pnpm-workspace.yaml`

- [ ] **Step 1: Create root Rust and pnpm workspace files**

Create `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = []

[workspace.package]
edition = "2021"
license = "UNLICENSED"
version = "0.1.0"
rust-version = "1.82"

[workspace.dependencies]
anyhow = "1"
chrono = { version = "0.4", features = ["serde"] }
rust_decimal = { version = "1", features = ["serde"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tauri = { version = "2" }
tauri-build = { version = "2" }
thiserror = "2"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
```

Create `package.json`:

```json
{
  "name": "evetools",
  "private": true,
  "version": "0.1.0",
  "scripts": {
    "dev": "pnpm --filter @evetools/desktop dev",
    "build": "pnpm --filter @evetools/desktop build",
    "typecheck": "pnpm --filter @evetools/desktop typecheck",
    "test:rust": "cargo test --workspace",
    "check": "cargo test --workspace && pnpm --filter @evetools/desktop typecheck"
  }
}
```

Create `pnpm-workspace.yaml`:

```yaml
packages:
  - "apps/*"
```

- [ ] **Step 2: Verify root workspace metadata**

Run: `cargo metadata --no-deps`

Expected: PASS and report an empty workspace package list.

- [ ] **Step 3: Commit scaffold**

```bash
git add Cargo.toml package.json pnpm-workspace.yaml
git commit -m "chore: add workspace roots"
```

## Task 2: Domain Crate With Price and Scoring Tests

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/domain/Cargo.toml`
- Create: `crates/domain/src/lib.rs`
- Create: `crates/domain/src/market.rs`
- Create: `crates/domain/src/scoring.rs`
- Create: `crates/domain/src/views.rs`
- Create: `crates/domain/src/fixtures.rs`

- [ ] **Step 1: Add the domain crate to the Rust workspace**

Modify the `members` block in `Cargo.toml`:

```toml
members = [
  "crates/domain"
]
```

- [ ] **Step 2: Create the domain crate manifest**

Create `crates/domain/Cargo.toml`:

```toml
[package]
name = "evetools-domain"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
rust_decimal.workspace = true
serde.workspace = true
```

- [ ] **Step 3: Write the domain module shell**

Create `crates/domain/src/lib.rs`:

```rust
pub mod fixtures;
pub mod market;
pub mod scoring;
pub mod views;

pub use market::{DataQuality, JITA_4_4_STATION_ID, OrderBookSummary, THE_FORGE_REGION_ID};
pub use scoring::{attention_score, gross_spread, liquidity_score, net_profit, FeeProfile};
pub use views::{MarketLookupView, OrderMonitorView, SelectionCandidateView};
```

- [ ] **Step 4: Write market model tests first**

Create `crates/domain/src/market.rs`:

```rust
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

pub const THE_FORGE_REGION_ID: i32 = 10000002;
pub const JITA_4_4_STATION_ID: i64 = 60003760;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataQuality {
    Fresh,
    Stale,
    Sparse,
    Missing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OrderBookSummary {
    pub type_id: i32,
    pub item_name: String,
    pub best_bid: Decimal,
    pub best_ask: Decimal,
    pub daily_volume: u64,
    pub top_buy_depth: u64,
    pub top_sell_depth: u64,
    pub last_synced_at: String,
}

impl OrderBookSummary {
    pub fn spread(&self) -> Decimal {
        self.best_ask - self.best_bid
    }

    pub fn spread_percent(&self) -> Decimal {
        if self.best_bid <= Decimal::ZERO {
            return Decimal::ZERO;
        }
        ((self.best_ask - self.best_bid) / self.best_bid) * Decimal::from(100)
    }

    pub fn data_quality(&self) -> DataQuality {
        if self.best_bid <= Decimal::ZERO || self.best_ask <= Decimal::ZERO {
            return DataQuality::Missing;
        }
        if self.daily_volume < 10 {
            return DataQuality::Sparse;
        }
        DataQuality::Fresh
    }

    pub fn rounded_spread_percent(&self) -> f64 {
        self.spread_percent().round_dp(2).to_f64().unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spread_percent_uses_best_bid_as_base() {
        let summary = OrderBookSummary {
            type_id: 34,
            item_name: "Tritanium".to_string(),
            best_bid: Decimal::new(500, 2),
            best_ask: Decimal::new(550, 2),
            daily_volume: 1_000_000,
            top_buy_depth: 50_000,
            top_sell_depth: 60_000,
            last_synced_at: "2026-05-25T12:00:00Z".to_string(),
        };

        assert_eq!(summary.spread(), Decimal::new(50, 2));
        assert_eq!(summary.rounded_spread_percent(), 10.0);
    }

    #[test]
    fn sparse_data_quality_requires_volume() {
        let summary = OrderBookSummary {
            type_id: 35,
            item_name: "Pyerite".to_string(),
            best_bid: Decimal::new(1000, 2),
            best_ask: Decimal::new(1300, 2),
            daily_volume: 3,
            top_buy_depth: 1,
            top_sell_depth: 1,
            last_synced_at: "2026-05-25T12:00:00Z".to_string(),
        };

        assert_eq!(summary.data_quality(), DataQuality::Sparse);
    }
}
```

- [ ] **Step 5: Write scoring implementation and tests**

Create `crates/domain/src/scoring.rs`:

```rust
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FeeProfile {
    pub sales_tax_rate: Decimal,
    pub broker_fee_rate: Decimal,
    pub order_modification_fee: Decimal,
}

impl FeeProfile {
    pub fn conservative_default() -> Self {
        Self {
            sales_tax_rate: Decimal::new(36, 3),
            broker_fee_rate: Decimal::new(30, 3),
            order_modification_fee: Decimal::ZERO,
        }
    }
}

pub fn gross_spread(best_bid: Decimal, best_ask: Decimal) -> Decimal {
    best_ask - best_bid
}

pub fn net_profit(best_bid: Decimal, best_ask: Decimal, fee: &FeeProfile) -> Decimal {
    let sale_after_tax = best_ask * (Decimal::ONE - fee.sales_tax_rate);
    let buy_with_broker = best_bid * (Decimal::ONE + fee.broker_fee_rate);
    sale_after_tax - buy_with_broker - fee.order_modification_fee
}

pub fn liquidity_score(daily_volume: u64, top_depth: u64) -> u8 {
    let volume_score = match daily_volume {
        0..=9 => 5,
        10..=99 => 25,
        100..=999 => 55,
        1_000..=9_999 => 80,
        _ => 100,
    };
    let depth_score = match top_depth {
        0..=4 => 10,
        5..=24 => 35,
        25..=99 => 60,
        100..=999 => 80,
        _ => 100,
    };
    ((volume_score + depth_score) / 2) as u8
}

pub fn attention_score(net_profit_margin_pct: Decimal, daily_volume: u64, top_depth: u64) -> u8 {
    let margin_score = if net_profit_margin_pct < Decimal::ZERO {
        0
    } else if net_profit_margin_pct < Decimal::new(2, 0) {
        25
    } else if net_profit_margin_pct < Decimal::new(5, 0) {
        55
    } else if net_profit_margin_pct < Decimal::new(12, 0) {
        80
    } else {
        100
    };
    let liquidity = liquidity_score(daily_volume, top_depth);
    ((margin_score as u16 * 60 + liquidity as u16 * 40) / 100) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn net_profit_subtracts_tax_broker_and_modification_fee() {
        let fee = FeeProfile {
            sales_tax_rate: Decimal::new(10, 2),
            broker_fee_rate: Decimal::new(5, 2),
            order_modification_fee: Decimal::new(25, 0),
        };

        let result = net_profit(Decimal::new(1000, 0), Decimal::new(1400, 0), &fee);

        assert_eq!(result, Decimal::new(185, 0));
    }

    #[test]
    fn liquidity_score_rejects_dead_items() {
        assert!(liquidity_score(1, 1) < 20);
        assert!(liquidity_score(2_500, 250) >= 80);
    }

    #[test]
    fn attention_score_balances_margin_and_liquidity() {
        let strong = attention_score(Decimal::new(8, 0), 2_500, 250);
        let weak = attention_score(Decimal::new(20, 0), 2, 1);

        assert!(strong > weak);
    }
}
```

- [ ] **Step 6: Write serialized view models**

Create `crates/domain/src/views.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketLookupView {
    pub type_id: i32,
    pub item_name: String,
    pub best_bid: String,
    pub best_ask: String,
    pub spread: String,
    pub spread_percent: String,
    pub daily_volume: u64,
    pub price_trend: String,
    pub top_buy_depth: u64,
    pub top_sell_depth: u64,
    pub last_synced_at: String,
    pub data_quality: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionCandidateView {
    pub type_id: i32,
    pub item_name: String,
    pub recommended_entry_price: String,
    pub recommended_exit_price: String,
    pub net_profit: String,
    pub attention_score: u8,
    pub liquidity_score: u8,
    pub confidence_score: u8,
    pub reason_codes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderMonitorView {
    pub order_id: String,
    pub type_id: i32,
    pub item_name: String,
    pub side: String,
    pub current_price: String,
    pub market_leader_price: String,
    pub recommended_price: String,
    pub recommended_action: String,
    pub urgency_score: u8,
    pub reason_codes: Vec<String>,
    pub stale_data_flag: bool,
}
```

- [ ] **Step 7: Write deterministic fixtures**

Create `crates/domain/src/fixtures.rs`:

```rust
use crate::views::{MarketLookupView, OrderMonitorView, SelectionCandidateView};

pub fn fixture_market_lookup(query: &str) -> MarketLookupView {
    let normalized = if query.trim().is_empty() {
        "Tritanium"
    } else {
        query.trim()
    };

    MarketLookupView {
        type_id: 34,
        item_name: normalized.to_string(),
        best_bid: "5.00".to_string(),
        best_ask: "5.50".to_string(),
        spread: "0.50".to_string(),
        spread_percent: "10.00".to_string(),
        daily_volume: 1_250_000,
        price_trend: "stable".to_string(),
        top_buy_depth: 500_000,
        top_sell_depth: 620_000,
        last_synced_at: "2026-05-25T12:00:00Z".to_string(),
        data_quality: "fresh".to_string(),
    }
}

pub fn fixture_selection_candidates() -> Vec<SelectionCandidateView> {
    vec![
        SelectionCandidateView {
            type_id: 34,
            item_name: "Tritanium".to_string(),
            recommended_entry_price: "5.01".to_string(),
            recommended_exit_price: "5.49".to_string(),
            net_profit: "0.23".to_string(),
            attention_score: 82,
            liquidity_score: 96,
            confidence_score: 88,
            reason_codes: vec![
                "healthy_spread".to_string(),
                "high_daily_volume".to_string(),
                "deep_top_book".to_string(),
            ],
        },
        SelectionCandidateView {
            type_id: 35,
            item_name: "Pyerite".to_string(),
            recommended_entry_price: "11.20".to_string(),
            recommended_exit_price: "12.05".to_string(),
            net_profit: "0.34".to_string(),
            attention_score: 68,
            liquidity_score: 77,
            confidence_score: 71,
            reason_codes: vec![
                "acceptable_spread".to_string(),
                "moderate_velocity".to_string(),
            ],
        },
    ]
}

pub fn fixture_order_monitor() -> Vec<OrderMonitorView> {
    vec![
        OrderMonitorView {
            order_id: "9000000001".to_string(),
            type_id: 34,
            item_name: "Tritanium".to_string(),
            side: "sell".to_string(),
            current_price: "5.60".to_string(),
            market_leader_price: "5.50".to_string(),
            recommended_price: "5.49".to_string(),
            recommended_action: "lower".to_string(),
            urgency_score: 91,
            reason_codes: vec![
                "undercut_detected".to_string(),
                "high_velocity_item".to_string(),
            ],
            stale_data_flag: false,
        },
        OrderMonitorView {
            order_id: "9000000002".to_string(),
            type_id: 35,
            item_name: "Pyerite".to_string(),
            side: "buy".to_string(),
            current_price: "11.10".to_string(),
            market_leader_price: "11.20".to_string(),
            recommended_price: "11.21".to_string(),
            recommended_action: "raise".to_string(),
            urgency_score: 76,
            reason_codes: vec!["overbid_detected".to_string()],
            stale_data_flag: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_include_all_three_mvp_views() {
        assert_eq!(fixture_market_lookup("Tritanium").item_name, "Tritanium");
        assert_eq!(fixture_selection_candidates().len(), 2);
        assert_eq!(fixture_order_monitor().len(), 2);
    }
}
```

- [ ] **Step 8: Run domain tests**

Run: `cargo test -p evetools-domain`

Expected: PASS, including tests named `spread_percent_uses_best_bid_as_base`, `net_profit_subtracts_tax_broker_and_modification_fee`, and `fixtures_include_all_three_mvp_views`.

- [ ] **Step 9: Commit domain crate**

```bash
git add Cargo.toml crates/domain
git commit -m "feat: add market domain foundation"
```

## Task 3: Supporting Rust Crates

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/esi/Cargo.toml`
- Create: `crates/esi/src/lib.rs`
- Create: `crates/db/Cargo.toml`
- Create: `crates/db/src/lib.rs`
- Create: `crates/worker/Cargo.toml`
- Create: `crates/worker/src/lib.rs`

- [ ] **Step 1: Add support crates to the Rust workspace**

Modify the `members` block in `Cargo.toml`:

```toml
members = [
  "crates/domain",
  "crates/esi",
  "crates/db",
  "crates/worker"
]
```

- [ ] **Step 2: Add ESI crate shell**

Create `crates/esi/Cargo.toml`:

```toml
[package]
name = "evetools-esi"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
thiserror.workspace = true
```

Create `crates/esi/src/lib.rs`:

```rust
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EsiError {
    #[error("ESI client is not connected in fixture mode")]
    FixtureMode,
}

pub fn client_mode() -> &'static str {
    "fixture"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esi_crate_starts_in_fixture_mode() {
        assert_eq!(client_mode(), "fixture");
        assert_eq!(EsiError::FixtureMode.to_string(), "ESI client is not connected in fixture mode");
    }
}
```

- [ ] **Step 3: Add DB crate shell**

Create `crates/db/Cargo.toml`:

```toml
[package]
name = "evetools-db"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
thiserror.workspace = true
```

Create `crates/db/src/lib.rs`:

```rust
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DbError {
    #[error("database is not initialized")]
    NotInitialized,
}

pub fn storage_mode() -> &'static str {
    "in-memory-fixture"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_crate_reports_fixture_storage() {
        assert_eq!(storage_mode(), "in-memory-fixture");
        assert_eq!(DbError::NotInitialized.to_string(), "database is not initialized");
    }
}
```

- [ ] **Step 4: Add worker crate shell**

Create `crates/worker/Cargo.toml`:

```toml
[package]
name = "evetools-worker"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
serde.workspace = true
```

Create `crates/worker/src/lib.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncStatus {
    pub public_market_sync: String,
    pub authenticated_order_sync: String,
}

pub fn fixture_sync_status() -> SyncStatus {
    SyncStatus {
        public_market_sync: "fixture-ready".to_string(),
        authenticated_order_sync: "not-authorized".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_reports_separate_public_and_private_sync_status() {
        let status = fixture_sync_status();
        assert_eq!(status.public_market_sync, "fixture-ready");
        assert_eq!(status.authenticated_order_sync, "not-authorized");
    }
}
```

- [ ] **Step 5: Run workspace Rust tests**

Run: `cargo test --workspace`

Expected: PASS for `evetools-domain`, `evetools-esi`, `evetools-db`, and `evetools-worker`; the desktop crate does not exist yet.

- [ ] **Step 6: Commit supporting crates**

```bash
git add Cargo.toml crates/esi crates/db crates/worker
git commit -m "chore: add rust support crates"
```

## Task 4: Tauri Desktop Command Adapter

**Files:**
- Modify: `Cargo.toml`
- Create: `apps/desktop/src-tauri/Cargo.toml`
- Create: `apps/desktop/src-tauri/build.rs`
- Create: `apps/desktop/src-tauri/tauri.conf.json`
- Create: `apps/desktop/src-tauri/capabilities/default.json`
- Create: `apps/desktop/src-tauri/src/main.rs`
- Create: `apps/desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: Add the Tauri crate to the Rust workspace**

Modify the `members` block in `Cargo.toml`:

```toml
members = [
  "crates/domain",
  "crates/esi",
  "crates/db",
  "crates/worker",
  "apps/desktop/src-tauri"
]
```

- [ ] **Step 2: Add Tauri crate manifest**

Create `apps/desktop/src-tauri/Cargo.toml`:

```toml
[package]
name = "evetools-desktop"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[lib]
name = "evetools_desktop_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build.workspace = true

[dependencies]
evetools-domain = { path = "../../../crates/domain" }
evetools-worker = { path = "../../../crates/worker" }
serde.workspace = true
serde_json.workspace = true
tauri.workspace = true
```

- [ ] **Step 3: Add Tauri build and config files**

Create `apps/desktop/src-tauri/build.rs`:

```rust
fn main() {
    tauri_build::build();
}
```

Create `apps/desktop/src-tauri/tauri.conf.json`:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "EveTools",
  "version": "0.1.0",
  "identifier": "com.evetools.desktop",
  "build": {
    "beforeDevCommand": "pnpm vite:dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "pnpm vite:build",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      {
        "title": "EVE Trader Assistant",
        "width": 1280,
        "height": 800,
        "minWidth": 1024,
        "minHeight": 720
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": "all"
  }
}
```

Create `apps/desktop/src-tauri/capabilities/default.json`:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Default desktop permissions",
  "windows": ["main"],
  "permissions": ["core:default"]
}
```

- [ ] **Step 4: Write command handlers**

Create `apps/desktop/src-tauri/src/lib.rs`:

```rust
use evetools_domain::fixtures::{
    fixture_market_lookup, fixture_order_monitor, fixture_selection_candidates,
};
use evetools_domain::{MarketLookupView, OrderMonitorView, SelectionCandidateView};
use evetools_worker::{fixture_sync_status, SyncStatus};

#[tauri::command]
pub fn lookup_market_price(query: String) -> Result<MarketLookupView, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err("Item query is required".to_string());
    }
    Ok(fixture_market_lookup(trimmed))
}

#[tauri::command]
pub fn list_selection_candidates() -> Result<Vec<SelectionCandidateView>, String> {
    Ok(fixture_selection_candidates())
}

#[tauri::command]
pub fn list_order_monitor_items() -> Result<Vec<OrderMonitorView>, String> {
    Ok(fixture_order_monitor())
}

#[tauri::command]
pub fn get_sync_status() -> Result<SyncStatus, String> {
    Ok(fixture_sync_status())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_rejects_empty_query() {
        let result = lookup_market_price("   ".to_string());
        assert_eq!(result.unwrap_err(), "Item query is required");
    }

    #[test]
    fn fixture_commands_return_mvp_views() {
        assert_eq!(lookup_market_price("Tritanium".to_string()).unwrap().item_name, "Tritanium");
        assert_eq!(list_selection_candidates().unwrap().len(), 2);
        assert_eq!(list_order_monitor_items().unwrap().len(), 2);
        assert_eq!(get_sync_status().unwrap().public_market_sync, "fixture-ready");
    }
}
```

Create `apps/desktop/src-tauri/src/main.rs`:

```rust
fn main() {
    evetools_desktop_lib::run();
}
```

- [ ] **Step 5: Run Rust tests including desktop command tests**

Run: `cargo test --workspace`

Expected: PASS, including `lookup_rejects_empty_query` and `fixture_commands_return_mvp_views`.

- [ ] **Step 6: Commit Tauri command adapter**

```bash
git add Cargo.toml apps/desktop/src-tauri
git commit -m "feat: add tauri command adapter"
```

## Task 5: React Desktop UI Shell

**Files:**
- Create: `apps/desktop/package.json`
- Create: `apps/desktop/index.html`
- Create: `apps/desktop/tsconfig.json`
- Create: `apps/desktop/vite.config.ts`
- Create: `apps/desktop/src/main.tsx`
- Create: `apps/desktop/src/App.tsx`
- Create: `apps/desktop/src/commands.ts`
- Create: `apps/desktop/src/styles.css`

- [ ] **Step 1: Add desktop package metadata**

Create `apps/desktop/package.json`:

```json
{
  "name": "@evetools/desktop",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "tauri dev",
    "build": "tauri build",
    "typecheck": "tsc --noEmit",
    "vite:dev": "vite --host 127.0.0.1 --port 1420",
    "vite:build": "vite build"
  },
  "dependencies": {
    "@tauri-apps/api": "^2.0.0",
    "@tanstack/react-table": "^8.21.3",
    "react": "^19.0.0",
    "react-dom": "^19.0.0"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.0.0",
    "@types/react": "^19.0.0",
    "@types/react-dom": "^19.0.0",
    "@vitejs/plugin-react": "^4.3.4",
    "typescript": "^5.8.0",
    "vite": "^6.0.0"
  }
}
```

- [ ] **Step 2: Add Vite and TypeScript config**

Create `apps/desktop/index.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>EVE Trader Assistant</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

Create `apps/desktop/tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "useDefineForClassFields": true,
    "lib": ["DOM", "DOM.Iterable", "ES2022"],
    "allowJs": false,
    "skipLibCheck": true,
    "esModuleInterop": true,
    "allowSyntheticDefaultImports": true,
    "strict": true,
    "forceConsistentCasingInFileNames": true,
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx"
  },
  "include": ["src"],
  "references": []
}
```

Create `apps/desktop/vite.config.ts`:

```ts
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true
  }
});
```

- [ ] **Step 3: Add typed command wrappers**

Create `apps/desktop/src/commands.ts`:

```ts
import { invoke } from "@tauri-apps/api/core";

export type MarketLookupView = {
  type_id: number;
  item_name: string;
  best_bid: string;
  best_ask: string;
  spread: string;
  spread_percent: string;
  daily_volume: number;
  price_trend: string;
  top_buy_depth: number;
  top_sell_depth: number;
  last_synced_at: string;
  data_quality: string;
};

export type SelectionCandidateView = {
  type_id: number;
  item_name: string;
  recommended_entry_price: string;
  recommended_exit_price: string;
  net_profit: string;
  attention_score: number;
  liquidity_score: number;
  confidence_score: number;
  reason_codes: string[];
};

export type OrderMonitorView = {
  order_id: string;
  type_id: number;
  item_name: string;
  side: string;
  current_price: string;
  market_leader_price: string;
  recommended_price: string;
  recommended_action: string;
  urgency_score: number;
  reason_codes: string[];
  stale_data_flag: boolean;
};

export type SyncStatus = {
  public_market_sync: string;
  authenticated_order_sync: string;
};

export function lookupMarketPrice(query: string): Promise<MarketLookupView> {
  return invoke<MarketLookupView>("lookup_market_price", { query });
}

export function listSelectionCandidates(): Promise<SelectionCandidateView[]> {
  return invoke<SelectionCandidateView[]>("list_selection_candidates");
}

export function listOrderMonitorItems(): Promise<OrderMonitorView[]> {
  return invoke<OrderMonitorView[]>("list_order_monitor_items");
}

export function getSyncStatus(): Promise<SyncStatus> {
  return invoke<SyncStatus>("get_sync_status");
}
```

- [ ] **Step 4: Add React entrypoint**

Create `apps/desktop/src/main.tsx`:

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
```

- [ ] **Step 5: Add desktop UI shell**

Create `apps/desktop/src/App.tsx`:

```tsx
import { useEffect, useState } from "react";
import {
  getSyncStatus,
  listOrderMonitorItems,
  listSelectionCandidates,
  lookupMarketPrice,
  type MarketLookupView,
  type OrderMonitorView,
  type SelectionCandidateView,
  type SyncStatus
} from "./commands";

type LoadState = "idle" | "loading" | "ready" | "error";

export default function App() {
  const [query, setQuery] = useState("Tritanium");
  const [lookup, setLookup] = useState<MarketLookupView | null>(null);
  const [candidates, setCandidates] = useState<SelectionCandidateView[]>([]);
  const [orders, setOrders] = useState<OrderMonitorView[]>([]);
  const [syncStatus, setSyncStatus] = useState<SyncStatus | null>(null);
  const [loadState, setLoadState] = useState<LoadState>("idle");
  const [error, setError] = useState<string | null>(null);

  async function refresh() {
    setLoadState("loading");
    setError(null);
    try {
      const [lookupResult, candidateResult, orderResult, statusResult] = await Promise.all([
        lookupMarketPrice(query),
        listSelectionCandidates(),
        listOrderMonitorItems(),
        getSyncStatus()
      ]);
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

  useEffect(() => {
    void refresh();
  }, []);

  return (
    <main className="app-shell">
      <header className="topbar">
        <div>
          <h1>EVE Trader Assistant</h1>
          <p>Jita 4-4 station trading cockpit</p>
        </div>
        <button type="button" onClick={() => void refresh()} disabled={loadState === "loading"}>
          {loadState === "loading" ? "Refreshing" : "Refresh"}
        </button>
      </header>

      <section className="status-row">
        <StatusCard label="Public market sync" value={syncStatus?.public_market_sync ?? "unknown"} />
        <StatusCard label="Order sync" value={syncStatus?.authenticated_order_sync ?? "unknown"} />
        <StatusCard label="Data source" value="fixture" />
      </section>

      {error && <div className="error-banner">{error}</div>}

      <section className="panel lookup-panel">
        <div className="panel-header">
          <h2>Market Price Lookup</h2>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void refresh();
            }}
          >
            <input value={query} onChange={(event) => setQuery(event.target.value)} aria-label="Item query" />
            <button type="submit">Lookup</button>
          </form>
        </div>
        {lookup && (
          <div className="metric-grid">
            <Metric label="Item" value={lookup.item_name} />
            <Metric label="Best bid" value={lookup.best_bid} />
            <Metric label="Best ask" value={lookup.best_ask} />
            <Metric label="Spread" value={`${lookup.spread} (${lookup.spread_percent}%)`} />
            <Metric label="Daily volume" value={lookup.daily_volume.toLocaleString()} />
            <Metric label="Data quality" value={lookup.data_quality} />
          </div>
        )}
      </section>

      <section className="dashboard-grid">
        <section className="panel">
          <div className="panel-header">
            <h2>Selection Discovery</h2>
            <span>{candidates.length} candidates</span>
          </div>
          <table>
            <thead>
              <tr>
                <th>Item</th>
                <th>Entry</th>
                <th>Exit</th>
                <th>Net</th>
                <th>Attention</th>
                <th>Reasons</th>
              </tr>
            </thead>
            <tbody>
              {candidates.map((candidate) => (
                <tr key={candidate.type_id}>
                  <td>{candidate.item_name}</td>
                  <td>{candidate.recommended_entry_price}</td>
                  <td>{candidate.recommended_exit_price}</td>
                  <td>{candidate.net_profit}</td>
                  <td>{candidate.attention_score}</td>
                  <td>{candidate.reason_codes.join(", ")}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>

        <section className="panel">
          <div className="panel-header">
            <h2>Order Monitor</h2>
            <span>{orders.length} orders</span>
          </div>
          <table>
            <thead>
              <tr>
                <th>Item</th>
                <th>Side</th>
                <th>Current</th>
                <th>Leader</th>
                <th>Recommended</th>
                <th>Urgency</th>
              </tr>
            </thead>
            <tbody>
              {orders.map((order) => (
                <tr key={order.order_id}>
                  <td>{order.item_name}</td>
                  <td>{order.side}</td>
                  <td>{order.current_price}</td>
                  <td>{order.market_leader_price}</td>
                  <td>{order.recommended_price}</td>
                  <td>{order.urgency_score}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      </section>
    </main>
  );
}

function StatusCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="status-card">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="metric">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}
```

- [ ] **Step 6: Add styling**

Create `apps/desktop/src/styles.css`:

```css
:root {
  color: #17212b;
  background: #eef2f5;
  font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  font-size: 15px;
  line-height: 1.4;
}

* {
  box-sizing: border-box;
}

body {
  margin: 0;
}

button,
input {
  font: inherit;
}

button {
  border: 1px solid #1f6f8b;
  background: #1f6f8b;
  color: #ffffff;
  border-radius: 6px;
  padding: 8px 12px;
  cursor: pointer;
}

button:disabled {
  cursor: wait;
  opacity: 0.7;
}

input {
  min-width: 240px;
  border: 1px solid #b8c4cf;
  border-radius: 6px;
  padding: 8px 10px;
}

.app-shell {
  min-height: 100vh;
  padding: 20px;
}

.topbar,
.panel-header,
.status-row,
.dashboard-grid,
form {
  display: flex;
  gap: 12px;
}

.topbar {
  align-items: center;
  justify-content: space-between;
  margin-bottom: 16px;
}

.topbar h1,
.panel h2 {
  margin: 0;
}

.topbar p {
  margin: 4px 0 0;
  color: #52616f;
}

.status-row {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  margin-bottom: 16px;
}

.status-card,
.panel {
  background: #ffffff;
  border: 1px solid #d9e1e8;
  border-radius: 8px;
  box-shadow: 0 1px 2px rgba(15, 23, 42, 0.06);
}

.status-card {
  padding: 12px;
}

.status-card span,
.metric span {
  display: block;
  color: #617080;
  font-size: 0.82rem;
}

.status-card strong,
.metric strong {
  display: block;
  margin-top: 4px;
}

.panel {
  padding: 16px;
}

.panel-header {
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}

.lookup-panel {
  margin-bottom: 16px;
}

.metric-grid {
  display: grid;
  grid-template-columns: repeat(6, minmax(0, 1fr));
  gap: 10px;
}

.metric {
  min-height: 70px;
  border: 1px solid #e1e8ef;
  border-radius: 6px;
  padding: 10px;
  background: #f8fafc;
}

.dashboard-grid {
  align-items: flex-start;
}

.dashboard-grid > .panel {
  flex: 1 1 0;
  min-width: 0;
}

table {
  width: 100%;
  border-collapse: collapse;
}

th,
td {
  border-bottom: 1px solid #e6edf3;
  padding: 9px 8px;
  text-align: left;
  vertical-align: top;
}

th {
  color: #52616f;
  font-size: 0.82rem;
  font-weight: 600;
}

.error-banner {
  margin-bottom: 16px;
  border: 1px solid #c2410c;
  border-radius: 8px;
  background: #fff7ed;
  color: #7c2d12;
  padding: 12px;
}

@media (max-width: 1080px) {
  .status-row,
  .metric-grid,
  .dashboard-grid {
    grid-template-columns: 1fr;
    display: grid;
  }

  .panel-header {
    align-items: flex-start;
    flex-direction: column;
  }
}
```

- [ ] **Step 7: Install JS dependencies**

Run: `pnpm install`

Expected: PASS and create `pnpm-lock.yaml`.

- [ ] **Step 8: Run frontend typecheck**

Run: `pnpm --filter @evetools/desktop typecheck`

Expected: PASS with no TypeScript errors.

- [ ] **Step 9: Run full check**

Run: `pnpm check`

Expected: PASS for `cargo test --workspace` and TypeScript typecheck.

- [ ] **Step 10: Commit desktop UI shell**

```bash
git add apps/desktop package.json pnpm-workspace.yaml pnpm-lock.yaml Cargo.toml
git commit -m "feat: add desktop ui foundation"
```

## Task 6: Manual Desktop Run Verification

**Files:**
- Modify only if verification finds a concrete issue in files created by Tasks 1-5.

- [ ] **Step 1: Start the desktop app**

Run: `pnpm dev`

Expected: Tauri opens a desktop window titled `EVE Trader Assistant`.

- [ ] **Step 2: Verify visible MVP surfaces**

Confirm the window shows:

- `Market Price Lookup`
- `Selection Discovery`
- `Order Monitor`
- Public market sync status `fixture-ready`
- Order sync status `not-authorized`

- [ ] **Step 3: Verify lookup interaction**

In the item query input, enter `Pyerite`, click `Lookup`, and confirm the Market Price Lookup card changes its item name to `Pyerite`.

- [ ] **Step 4: Stop the app**

Stop the `pnpm dev` process with `Ctrl+C`.

- [ ] **Step 5: Commit verification fixes if any were needed**

If no fixes were needed, skip this commit. If fixes were needed, commit them:

```bash
git add apps/desktop crates
git commit -m "fix: stabilize desktop foundation"
```

## Task 7: Update Project Notes

**Files:**
- Create: `README.md`

- [ ] **Step 1: Add a root README**

Create `README.md`:

````markdown
# EveTools

EveTools is a desktop-first EVE Online station trading assistant focused on Jita 4-4.

The first implementation slice provides:

- Tauri 2 desktop shell
- React/Vite UI
- Rust domain calculations
- Fixture-backed market price lookup
- Fixture-backed selection discovery
- Fixture-backed order monitor

## Development

Install dependencies:

```powershell
pnpm install
```

Run checks:

```powershell
pnpm check
```

Start the desktop app:

```powershell
pnpm dev
```

## Architecture

Business logic lives in Rust crates:

- `crates/domain`
- `crates/esi`
- `crates/db`
- `crates/worker`

The desktop app lives in `apps/desktop`. React renders the UI and calls Tauri commands. Tauri commands are adapters; trading calculations stay in `crates/domain`.
````

- [ ] **Step 2: Commit README**

```bash
git add README.md
git commit -m "docs: add development notes"
```

## Final Verification

- [ ] **Step 1: Run all checks**

Run: `pnpm check`

Expected: PASS.

- [ ] **Step 2: Review git status**

Run: `git status --short`

Expected: no unexpected modified files. `pnpm-lock.yaml` should be committed if generated by `pnpm install`.

## Self-Review Notes

Spec coverage for this first slice:

- Tauri desktop app shell: Task 4 and Task 5.
- Rust core boundary: Task 2, Task 3, Task 4.
- Market Price Lookup: Task 2 fixture model, Task 4 command, Task 5 UI.
- Selection Discovery: Task 2 fixture model, Task 4 command, Task 5 UI.
- Order Monitor: Task 2 fixture model, Task 4 command, Task 5 UI.
- Real public ESI sync: deferred to the next implementation plan.
- Real SQLite persistence: deferred to the next implementation plan.
- Real EVE SSO and authenticated order sync: deferred to a later implementation plan after public ESI sync.
