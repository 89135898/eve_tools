# NPC Hub Selection Discovery Design

Date: 2026-05-26

## Purpose

Replace the fixed Selection Discovery seed list with automatic station-trading recommendations from real public market orders at the main NPC trade hubs.

The first version should discover station-trading opportunities from market state itself. It should not require a manually maintained candidate pool, and it should not attempt player-structure markets or cross-station hauling arbitrage.

## Confirmed Decisions

- Selection Discovery recommends station-internal trading opportunities, not hauling routes.
- The first version covers major NPC trade stations only.
- Perimeter and other player-owned structure markets are out of scope for this version.
- A candidate pool is not a product input for discovery. The discovery pipeline derives items from orders seen at configured trade stations.
- Static EVE data is still needed, but as metadata for names, filtering, categories, and display, not as the primary candidate source.

## Trade Hub Scope

The first version supports these NPC trade stations:

| Hub | Region | Region ID | System | System ID | Station | Station ID |
| --- | --- | ---: | --- | ---: | --- | ---: |
| Jita | The Forge | 10000002 | Jita | 30000142 | Jita IV - Moon 4 - Caldari Navy Assembly Plant | 60003760 |
| Amarr | Domain | 10000043 | Amarr | 30002187 | Amarr VIII (Oris) - Emperor Family Academy | 60008494 |
| Dodixie | Sinq Laison | 10000032 | Dodixie | 30002659 | Dodixie IX - Moon 20 - Federation Navy Assembly Plant | 60011866 |
| Rens | Heimatar | 10000030 | Rens | 30002510 | Rens VI - Moon 8 - Brutor Tribe Treasury | 60004588 |
| Hek | Metropolis | 10000042 | Hek | 30002053 | Hek VIII - Moon 12 - Boundless Creation Factory | 60005686 |

These station, system, and region IDs were verified through public ESI station, system, and constellation lookup. The implementation should keep them in a domain or worker configuration module with tests, not inline in Tauri command handlers.

## Product Behavior

Selection Discovery should show a ranked list of opportunities for one or more selected hubs.

Each recommendation should answer:

- Which station is this recommendation for?
- What item is being recommended?
- What is the current best buy and best sell price at that station?
- What entry and exit prices would a station trader likely use?
- What is the estimated net profit after fees?
- How liquid does the item look?
- How reliable is the recommendation?
- Why did this item surface?

The initial UI can keep the existing Selection Discovery table shape, but rows need a hub field and filtering by hub. A later UI pass can add tabs or a hub selector if the result set grows large.

## Data Sources

### Public ESI Market Orders

Use unauthenticated region market orders:

- `GET /markets/{region_id}/orders/`

For each configured region, fetch all pages for order type `all`. Then filter locally by the configured NPC station IDs.

Region-level fetching is necessary because public NPC station orders are exposed through region order pages. The worker should group configured hubs by region so one region fetch can serve every configured station in that region.

### Public ESI Market History

Use market history only after order-book preselection:

- `GET /markets/{region_id}/history/`

History is per region and type, not per station. It should be used as a regional liquidity and trend proxy for station recommendations, while station-specific price, spread, and depth must come from station-filtered orders. Pulling history for every item observed in all hub orders would multiply request volume unnecessarily. The first version should:

1. Build a cheap order-book score from station orders.
2. Keep a bounded preselection set per hub.
3. Fetch history only for the top preselected type IDs.
4. Re-rank using volume, trend, and confidence.

### Static EVE Data

Use local static data for item metadata:

- type ID
- English and Chinese names when available
- group and category
- market group
- volume and packaged volume when available
- published or market-eligible flags when available

The first implementation can fall back to ESI type lookup for display names if SDE import is not ready, but the target architecture should use local static metadata. This keeps discovery deterministic and avoids one ESI type-info request per market item.

### Player Structures

Player-structure markets are out of scope because they require the authenticated structure market flow and structure IDs. The NPC-hub discovery design should not depend on `GET /markets/structures/{structure_id}/`.

## Synchronization Design

Introduce a public market sync service that can refresh configured NPC hubs.

Suggested layers:

```text
React UI
  |
Tauri commands
  |
Discovery application service
  |
Public market sync worker
  |
ESI client + SQLite repositories
  |
Domain ranking engine
```

The Tauri command should request recommendations, not run the synchronization logic inline. If cached recommendations are fresh enough, return them immediately. If data is missing or stale, trigger a refresh and return either a loading state, fixture fallback, or the latest cached result with stale status.

## Fetch Strategy

For each refresh run:

1. Load configured hubs and group them by region ID.
2. Fetch all market order pages for each needed region.
3. Store a sync run record with start time, end time, ESI cache headers where available, page count, status, and error summary.
4. Filter raw orders to configured NPC station IDs.
5. Store or replace the current order snapshot for the refresh run.
6. Aggregate station order books by `(hub_id, type_id)`.
7. Compute cheap preselection scores from order-book data.
8. Select a bounded number of type IDs per hub for history enrichment.
9. Fetch market history for those type IDs.
10. Compute final recommendation scores and persist recommendation snapshots.

The default preselection limit should be conservative, for example 100 items per hub before history enrichment and 50 final recommendations per hub. These values should be settings or constants with tests, not hidden magic numbers.

## Rate Limit and Cache Handling

The worker should be built around ESI cache behavior and rate limits:

- Respect response cache headers and avoid refreshing a region before its cached market-order data is expected to change.
- Store sync status so refreshes are single-flight per region. Multiple UI requests should join or reuse the same refresh rather than start duplicate region scans.
- Treat partial region fetch failure as stale or degraded data, not as a reason to mix incomplete pages into a fresh recommendation set.
- Record page counts and page failures so the UI can show degraded sync state later.
- Keep fixture mode available for deterministic tests and offline development.

The first version does not need background scheduling, but the service boundary should allow adding scheduled refresh later.

## Storage Model Direction

The design needs SQLite persistence because full region order scans are too expensive to repeat for every UI refresh.

Suggested tables:

- `trade_hubs`
- `market_sync_runs`
- `market_order_snapshots`
- `market_history_snapshots`
- `item_metadata`
- `selection_recommendation_snapshots`
- `app_settings`

Minimum fields:

### `trade_hubs`

- `hub_id`
- `display_name`
- `region_id`
- `system_id`
- `station_id`
- `enabled`
- `sort_order`

### `market_sync_runs`

- `sync_run_id`
- `region_id`
- `started_at`
- `completed_at`
- `status`
- `page_count`
- `error_summary`
- `source`

### `market_order_snapshots`

- `sync_run_id`
- `region_id`
- `station_id`
- `type_id`
- `order_id`
- `is_buy_order`
- `price`
- `volume_remain`
- `issued`
- `duration`
- `min_volume`
- `range`

### `market_history_snapshots`

- `region_id`
- `type_id`
- `date`
- `average`
- `highest`
- `lowest`
- `order_count`
- `volume`
- `fetched_at`

### `selection_recommendation_snapshots`

- `sync_run_id`
- `hub_id`
- `station_id`
- `type_id`
- `item_name`
- `best_bid`
- `best_ask`
- `recommended_entry_price`
- `recommended_exit_price`
- `net_profit`
- `attention_score`
- `liquidity_score`
- `confidence_score`
- `regional_daily_volume`
- `reason_codes`
- `last_synced_at`
- `data_quality`

The first implementation may start with a narrower schema, but these concepts should not be collapsed into fixture-only view construction.

## Domain Model Changes

The current domain function is Jita-specific. Replace the station-specific shape with station-parameterized analysis.

Recommended concepts:

- `TradeHub`
- `HubOrderBookSummary`
- `HubSelectionRecommendation`
- `DiscoveryPreselection`
- `DiscoveryScore`

The current `summarize_jita_market` behavior should become a generic function that accepts station ID and type ID. Jita can remain one configured hub rather than a special-case function.

The recommendation should include both station identity and item identity:

```text
hub_id
hub_name
station_id
region_id
type_id
item_name
best_bid
best_ask
recommended_entry_price
recommended_exit_price
net_profit
attention_score
liquidity_score
confidence_score
regional_daily_volume
reason_codes
```

## Ranking Model

The first version should score station-internal trading opportunities with a transparent, testable formula.

Primary positive signals:

- positive fee-adjusted net spread
- enough daily regional volume
- enough depth at or near top of book
- enough buy and sell order count
- recent history is available
- spread is not caused by a missing side

Primary negative signals:

- missing buy or sell side
- negative net profit after fees
- very low historical volume
- very shallow top book
- extreme spread with very low volume
- stale or partial sync data

Reason codes should be stable strings so React can localize them:

- `healthy_spread`
- `acceptable_spread`
- `high_daily_volume`
- `moderate_velocity`
- `deep_top_book`
- `thin_top_book`
- `missing_market_side`
- `sparse_market_data`
- `stale_market_data`
- `negative_net_profit`
- `extreme_spread_low_volume`
- `partial_sync`

The first formula can reuse existing `attention_score`, `liquidity_score`, and fee calculations, but must add hub-aware inputs and explicit degraded-data handling.

## API and UI Direction

Replace the current fixed-seed command behavior behind:

- `list_selection_candidates`

The command can keep its frontend name initially to avoid broad UI churn, but the returned rows should come from discovery snapshots instead of `SELECTION_SEED_TYPES`.

Future-friendly command shape:

```text
list_selection_recommendations(filter)
```

Filter fields:

- enabled hub IDs
- max rows per hub
- minimum attention score
- include stale results

View rows should add:

- `hub_id`
- `hub_name`
- `station_id`
- `region_id`
- `last_synced_at`
- `sync_status`

The UI should show fixture, live, stale, and degraded states distinctly. Existing localization resources need new labels for hub names, sync states, and reason codes.

## Error Handling

Expected error classes:

- ESI region-order fetch failure
- ESI market-history fetch failure
- partial page fetch failure
- static metadata missing for a type ID
- SQLite read or write failure
- sync already running

Behavior:

- If all live order fetches fail, return fixture fallback in fixture-compatible development mode or a live error state in strict live mode.
- If one region fails and others succeed, return successful hubs and mark failed hubs as degraded or stale.
- If metadata is missing, keep the type ID in the recommendation with a fallback item name like `Type 34`, and mark a metadata reason internally.
- Do not silently turn item-not-found or missing metadata into unrelated fixture recommendations.

## Testing Strategy

Domain tests:

- station-parameterized order summary filters by station and type ID
- multi-hub aggregation keeps the same type separate by hub
- recommendations exclude missing-side books from high-confidence results
- fee-adjusted net profit drives score ordering
- low-volume extreme spreads are penalized
- reason codes are stable

ESI client tests:

- region order pagination reads all pages
- page failure maps to a recoverable fetch error
- history fetch remains per `(region_id, type_id)`
- recommendation views label history-derived volume as regional volume, not station volume

DB tests:

- sync runs persist status and page counts
- order snapshots can be replaced by sync run
- recommendation snapshots can be queried by hub and score

Desktop command tests:

- fixture source still returns deterministic rows
- live service returns hub-aware recommendations from mocked repositories or mocked ESI
- partial hub failure returns degraded status without losing successful hubs

Frontend tests or type checks:

- new row fields are typed
- hub labels and reason codes are localized
- empty, loading, stale, degraded, and fixture states render without crashing

## Migration From Current Implementation

Current behavior:

- `SELECTION_SEED_TYPES` hard-codes four minerals.
- `list_selection_candidates` fetches per seed item.
- `summarize_jita_market` filters only Jita 4-4.
- Selection Discovery rows do not include hub identity.

Target behavior:

- Remove `SELECTION_SEED_TYPES` as product input.
- Configure major NPC trade hubs in Rust.
- Fetch region order pages once per required region.
- Filter orders to configured station IDs.
- Aggregate by `(hub, type_id)`.
- Enrich top-ranked item groups with history.
- Return hub-aware recommendation rows.

This can be implemented incrementally while keeping fixture fallback and the existing UI table available.

## Out of Scope

- Player-structure markets such as Perimeter.
- Cross-station hauling arbitrage.
- Authenticated character-order monitoring.
- Automated order placement, modification, or cancellation.
- Full all-region market database.
- Background daemon scheduling beyond the service boundary.
- Perfect SDE coverage for industry, dogma, blueprints, or universe geography.

## Open Implementation Choices

These choices can be resolved in the implementation plan:

- Whether SQLite schema lands before or together with the discovery service.
- Whether the first static metadata source is a small bundled fixture, ESI type lookup cache, or SDE import subset.
- The exact preselection and final recommendation limits.
- Whether the UI first uses a hub filter dropdown or grouped sections.

The design requires that these choices keep the product behavior candidate-free from the user's perspective.
