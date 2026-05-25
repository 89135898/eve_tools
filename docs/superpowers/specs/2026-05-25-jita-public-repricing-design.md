# Jita Public Repricing Assistant Design

Date: 2026-05-25

Status: Superseded by `docs/superpowers/specs/2026-05-25-jita-two-board-station-trading-mvp-design.md`.

This draft is kept for historical context. Do not use it as the implementation source of truth.

## Purpose

Build a local tool that analyzes public Jita market data and recommends how a trader should update prices for watched items. This first version does not manage personal orders, does not require EVE login, and does not store cloud accounts.

The goal is to validate whether a multi-factor repricing model is useful before adding private data, character-level order management, or any account system.

## Product Scope

This version is a public-market-only assistant for Jita 4-4.

In scope:

- Public Jita order book analysis
- Public Jita price history analysis
- Watchlist-based item tracking
- Buy-side and sell-side repricing suggestions
- Local configuration and local history cache
- Ranked alerts for items that need attention

Out of scope:

- EVE SSO login
- Personal orders, wallet, assets, or transactions
- Auto-order placement or modification
- Multi-station routing
- Non-Jita regions
- Cloud sync or multi-user accounts

## User Outcome

The tool should answer these questions:

1. What is the current Jita market state for the items I care about?
2. If I want to refresh a quote, what price should I move to?
3. Which items are drifting, slowing down, or becoming uncompetitive?
4. Which items should I ignore for now?

## Recommended MVP

The first release should include five core flows:

1. **Watchlist setup**
   - Add items by name or type id.
   - Mark each item as buy-side, sell-side, or both.
   - Store local notes and priority flags.

2. **Market sync**
   - Fetch public Jita order book data for watched items.
   - Fetch public price history for watched items.
   - Respect ESI cache and rate-limiting guidance.
   - Cache results locally with timestamps.

3. **Repricing recommendation**
   - Recommend a target buy price, sell price, or both.
   - Provide a confidence score and reason codes.
   - Show whether the suggestion is aggressive, balanced, or conservative.

4. **Analysis view**
   - Show current best bid/ask, spread, depth near top of book, recent volume, recent volatility, and recommendation outcome.
   - Sort items by attention score.

5. **Local settings**
   - Configure station focus, refresh cadence, minimum spread, minimum volume, and preferred aggressiveness.
   - Persist everything locally.

## Recommendation Model

The repricing engine should combine several signals instead of relying on a single spread check.

Core signals:

- Best bid and best ask in Jita
- Spread percentage
- Order depth near the top of book
- Recent trade volume
- Short-term price movement
- Short-term price volatility
- Order concentration near the top
- Item-specific minimum tick or step size

Suggested outputs:

- `recommended_price`
- `recommended_action` (`raise`, `lower`, `hold`, `enter`, `exit`)
- `attention_score`
- `confidence_score`
- `reason_codes`

The initial algorithm does not need to be clever. It only needs to be explainable and stable. A trader should be able to read the reasons and understand why the tool produced the suggestion.

## Architecture

Keep the feature aligned with the architecture baseline:

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
Public ESI
```

Suggested module split:

```text
apps/
  api/              market sync, local config, recommendation endpoints
  web/              dashboard, tables, charts, settings
packages/
  domain/           repricing model, scoring, item ranking
  esi/              public ESI client and response normalization
  db/               storage and query layer
```

No module in this phase should depend on EVE auth or private character data.

## Data Flow

1. The user adds watched items in the UI.
2. The backend resolves item metadata and stores the watchlist locally.
3. A sync job fetches public market orders and public history for those items.
4. The backend stores snapshots and timestamps.
5. Domain services compute repricing suggestions and attention scores.
6. The UI renders the ranked results and item detail panels.

The UI should always show freshness. If data is stale, the recommendation should be marked as stale rather than silently reused.

## Data Model Direction

The schema should include at least these concepts:

- `watchlist_items`
- `item_metadata`
- `market_order_snapshots`
- `market_history_snapshots`
- `recommendation_snapshots`
- `sync_runs`
- `app_settings`

Historical snapshots matter because repricing depends on trends, not only the current top of book.

## Reliability Rules

- Never treat one market snapshot as enough for a strong recommendation when the data is stale or incomplete.
- Respect ESI cache headers and endpoint-specific rate limits.
- Validate every remote response before storing it.
- Show sync failures and stale-data states explicitly.
- Keep price calculations out of floating-point arithmetic where precision matters.
- Store settings locally and avoid a cloud dependency in the MVP.

## Error Handling

The product should handle these cases explicitly:

- Item lookup fails
- Market data for a watched item is missing
- ESI returns a transient error or rate limit response
- A recommendation cannot be computed with enough confidence
- Local cache is stale or partially populated

In those cases, the UI should show an explanation and the last known good data when available.

## Testing Strategy

The first implementation plan should cover:

- Unit tests for the repricing model
- Fixture-based tests for ESI normalization
- Tests for item ranking and confidence scoring
- API tests for watchlist, sync, and recommendation endpoints
- Basic UI smoke tests for the watchlist and recommendation table

The recommendation engine is the highest-risk part of the feature and needs the strongest test coverage.

## Deferred Decisions

These items are intentionally left for later:

- EVE SSO and private order support
- Personal order synchronization
- Account management
- Cloud sync
- Multiple regions beyond Jita
- Full market scanning beyond the watchlist
- Automated order placement or modification
- Desktop packaging

## Success Criteria

The first version is useful if it can:

- Load a watchlist of Jita items
- Refresh market data locally
- Produce understandable repricing suggestions
- Rank items by urgency
- Keep its recommendations explainable and stable

If the tool cannot explain why it recommends a price, it is not ready.
