# Jita Two-Board Station Trading MVP Design

Date: 2026-05-25

## Purpose

Build a local EVE market tool focused on Jita 4-4 station trading with two core boards:

1. **Selection Discovery**: find items worth trading.
2. **Order Monitor**: track the user's open orders and flag when they need repricing.

These boards sit on top of a shared **Market Price Lookup** capability. Price lookup is not a separate product mode in the MVP; it is the reusable foundation for item search, price cache maintenance, item detail pages, and recommendation explanations.

This document supersedes the earlier public-only repricing draft. The new MVP keeps the useful parts of that idea, but adds authenticated order monitoring, shared price lookup, and separates public market analysis from private character data.

## Product Scope

This version is a single-user local tool.

In scope:

- Jita market price lookup
- Jita-focused selection discovery
- Public market order analysis
- Public market history analysis
- SSO-based character order monitoring
- Local watchlists and fee profiles
- Local snapshots, alerts, and ranking

Out of scope:

- Automated order placement or cancellation
- Wallet, assets, industry, or hauling workflows
- Cloud accounts or multi-user sharing
- Multi-region expansion
- Corporation-level analysis
- Desktop packaging

## User Outcomes

The tool should answer two questions quickly:

1. Which items in Jita look worth trading right now?
2. Which of my open orders need a price change, and what should I move them to?

It should also answer the supporting question:

3. What is the current Jita price state for a specific item?

## Shared Capability: Market Price Lookup

Market Price Lookup is the foundation used by both boards.

Primary tasks:

- Search items by name or type id.
- Show current Jita best bid and best ask.
- Show spread, recent volume, and recent price movement.
- Show top-of-book depth for buy and sell sides.
- Show data freshness and sync status.
- Let the user add an item to a watchlist or candidate pool.

Recommended MVP behavior:

- The lookup result should be read-only market intelligence, not a trading recommendation by itself.
- It should reuse the same cached market snapshots as Selection Discovery and Order Monitor.
- It should support direct lookup even when the item is not currently in the candidate pool.
- It should make it clear when the item has sparse, stale, or incomplete data.

Suggested outputs:

- `best_bid`
- `best_ask`
- `spread`
- `daily_volume`
- `price_trend`
- `top_buy_depth`
- `top_sell_depth`
- `last_synced_at`
- `data_quality`

## MVP Boards

### 1. Selection Discovery

This board is for finding candidate items.

Primary tasks:

- Compare best bid and best ask in Jita.
- Estimate gross spread and net spread.
- Use market history to estimate velocity and liquidity.
- Ignore items that look profitable but are too slow to matter.
- Rank items by attention score and explain why they surfaced.

Recommended MVP input model:

- A configurable candidate pool rather than a full blind scan of every tradeable item by default.
- The pool can be seeded from saved watchlists, category filters, top-volume items, and search-based additions.
- Full region-wide item crawling is deferred until the market sync pipeline and rate-limit handling prove stable.

Core signals:

- Best bid
- Best ask
- Gross spread
- Estimated net profit
- Daily volume
- Short-term volatility
- Order depth near the top of book
- Item-specific tick size or meaningful price step

Suggested outputs:

- `recommended_entry_price`
- `recommended_exit_price`
- `attention_score`
- `liquidity_score`
- `confidence_score`
- `reason_codes`

### 2. Order Monitor

This board is for the user's own active orders.

Primary tasks:

- Log in with EVE SSO.
- Load the character's current open market orders.
- Compare each order against current Jita market conditions.
- Detect undercut sell orders and overbid buy orders.
- Recommend the next price move and show urgency.

Required scope:

- `esi-markets.read_character_orders.v1`

This board does not need wallet data in the MVP.

Suggested outputs:

- `recommended_price`
- `recommended_action` (`raise`, `lower`, `hold`)
- `urgency_score`
- `reason_codes`
- `stale_data_flag`

## Fee Model

The selection board needs a configurable fee profile because net profit depends on the trader's skills and standing.

The MVP should support manual input for:

- Accounting level
- Broker Relations level
- Relevant standings or a simplified broker fee override
- Sales tax override if needed

The tool should not hard-code a single fee formula without user-adjustable inputs. If a fee cannot be derived confidently, the UI should mark the result as estimate-based.

## Architecture

Keep the same backend-driven Web Dashboard shape, but split public and authenticated flows:

```text
React Web Dashboard
        |
Fastify API
        |
Application Services
        |
Domain Engine
        |
SQLite cache
        |
Public ESI / Authenticated ESI
```

Suggested module split:

```text
apps/
  api/              price lookup, public sync, SSO callback, order monitor, recommendations
  web/              price lookup, selection board, monitor board, settings
packages/
  domain/           spread, liquidity, profitability, urgency, ranking
  esi/              public and authenticated ESI clients
  db/               local storage, snapshots, repositories
  shared/           view models and shared types
```

The two boards should share the same domain engine, but the public-selection pipeline and the authenticated-order pipeline must remain separate at the transport layer.

## Data Flow

### Market Price Lookup Flow

1. The user searches for an item.
2. The backend resolves the item id and metadata.
3. The backend reads fresh cached Jita data if available.
4. If cache is missing or stale, the backend schedules or performs a public market sync.
5. Domain services compute spread, depth, volume, trend, and data-quality fields.
6. The UI shows a read-only market detail view and actions to add the item to a watchlist or candidate pool.

### Selection Discovery Flow

1. The user defines a candidate pool.
2. The backend resolves item metadata.
3. The sync job fetches public Jita market orders and market history.
4. The backend stores snapshots locally.
5. Domain services compute spread, volume, liquidity, and net-profit estimates.
6. The UI ranks candidates and shows reasons.

### Order Monitor Flow

1. The user authorizes the app through EVE SSO.
2. The backend stores the refresh token locally.
3. A sync job loads the character's open orders.
4. The backend compares them to current Jita conditions.
5. Domain services compute repricing urgency and a target price.
6. The UI highlights orders that need attention.

## Data Model Direction

The schema should include these concepts:

- `candidate_items`
- `watchlist_items`
- `item_metadata`
- `price_lookup_history`
- `fee_profiles`
- `market_order_snapshots`
- `market_history_snapshots`
- `character_order_snapshots`
- `recommendation_snapshots`
- `sync_runs`
- `app_settings`
- `auth_tokens`
- `characters`

Historical snapshots matter because both item selection and order monitoring depend on change over time, not only the latest top-of-book state.

## Reliability Rules

- Respect ESI cache headers and rate limits.
- Separate public data freshness from authenticated data freshness.
- Show stale-data flags instead of silently reusing old recommendations.
- Validate all ESI responses before persistence.
- Keep all money calculations out of floating-point arithmetic where precision matters.
- Store refresh tokens securely in local storage only; never expose them to the browser.

## Error Handling

The product should explicitly handle:

- Item lookup failures
- Price lookup with stale cached data
- Missing market history
- Sparse or stale market data
- SSO cancellation
- Token refresh failure
- Order data that cannot be confidently repriced
- Sync jobs that partially succeed

The UI should always show whether the recommendation is fresh, estimated, or degraded.

## Testing Strategy

The implementation plan should include:

- Unit tests for spread, liquidity, net-profit, and urgency scoring
- Fixture-based tests for public market normalization
- Fixture-based tests for authenticated character-order normalization
- API tests for price lookup, candidate sync, SSO flow, and monitor endpoints
- UI smoke tests for price lookup, both boards, and the settings screen

The ranking model is the highest-risk part of the MVP and needs direct test coverage.

## Deferred Decisions

These are intentionally out of scope for the first version:

- Full region-wide crawling of every tradeable item
- Wallet-based realized profit analysis
- Automatic relisting or undercutting
- Multi-character support
- Cloud sync and account management
- Corporation or alliance overlays
- Manufacturing and hauling analytics
- Desktop packaging

## Success Criteria

The first version is useful if it can:

- Look up the current Jita price state for a specific item
- Surface a sensible list of Jita trading candidates
- Show why each candidate ranks well
- Detect when a character's orders need repricing
- Give a target price and urgency signal
- Keep public selection and private order monitoring clearly separated

If the tool cannot explain why a candidate or order was ranked, it is not ready.
