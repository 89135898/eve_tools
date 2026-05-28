use evetools_db::{
    connect_pool, migrate_catalog_schema, CatalogDbError, CatalogRepository, CatalogStatus,
    InventoryTypeView, MarketDbError, MarketOrderSnapshot, MarketRepository, StationOrderBook,
    TradeHub,
};
use evetools_domain::{
    build_selection_candidate, FeeProfile, OrderBookSummary, SelectionCandidateHubView,
    SelectionCandidateView,
};
use rust_decimal::{prelude::FromPrimitive, Decimal};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use thiserror::Error;

pub use evetools_db::{
    CatalogStatus as CatalogStatusView, InventoryTypeView as InventoryTypeApiView,
    MarketOrderSnapshot as MarketOrderView, TradeHub as TradeHubView,
};

const MAX_SELECTION_CANDIDATE_LIMIT: usize = 100;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("sql connection error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("sql migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("catalog query error: {0}")]
    Catalog(#[from] CatalogDbError),
    #[error("market query error: {0}")]
    Market(#[from] MarketDbError),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryTypeLookupRequest {
    pub type_id: i32,
    pub language: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryTypeSearchRequest {
    pub query: String,
    pub language: String,
    pub limit: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StationOrdersRequest {
    pub region_id: i32,
    pub station_id: i64,
    pub limit: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionCandidatesRequest {
    pub hub_ids: Vec<String>,
    pub language: String,
    pub limit_per_hub: i64,
}

#[derive(Clone)]
pub struct EveToolsReadApi {
    catalog: CatalogRepository,
    market: MarketRepository,
}

impl EveToolsReadApi {
    pub async fn connect(database_url: &str) -> Result<Self, ApiError> {
        let pool = connect_pool(database_url).await?;
        migrate_catalog_schema(&pool).await?;
        Ok(Self::from_pool(pool))
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self {
            catalog: CatalogRepository::new(pool.clone()),
            market: MarketRepository::new(pool),
        }
    }

    pub async fn catalog_status(&self) -> Result<CatalogStatus, ApiError> {
        Ok(self.catalog.latest_status().await?)
    }

    pub async fn get_inventory_type(
        &self,
        request: InventoryTypeLookupRequest,
    ) -> Result<Option<InventoryTypeView>, ApiError> {
        Ok(self
            .catalog
            .get_inventory_type(request.type_id, &request.language)
            .await?)
    }

    pub async fn search_inventory_types(
        &self,
        request: InventoryTypeSearchRequest,
    ) -> Result<Vec<InventoryTypeView>, ApiError> {
        Ok(self
            .catalog
            .search_inventory_types(&request.query, &request.language, request.limit)
            .await?)
    }

    pub async fn list_trade_hubs(&self) -> Result<Vec<TradeHub>, ApiError> {
        Ok(self.market.list_enabled_trade_hubs().await?)
    }

    pub async fn latest_station_orders(
        &self,
        request: StationOrdersRequest,
    ) -> Result<Vec<MarketOrderSnapshot>, ApiError> {
        Ok(self
            .market
            .latest_station_orders(request.region_id, request.station_id, request.limit)
            .await?)
    }

    pub async fn selection_candidates(
        &self,
        request: SelectionCandidatesRequest,
    ) -> Result<Vec<SelectionCandidateView>, ApiError> {
        let Some(limit_per_hub) = selection_candidate_limit(request.limit_per_hub) else {
            return Ok(Vec::new());
        };
        let requested_hub_ids = request.hub_ids;
        let hubs = self.market.list_enabled_trade_hubs().await?;
        let fee = FeeProfile::conservative_default();
        let fetch_limit = i64::try_from(limit_per_hub.saturating_mul(5)).unwrap_or(i64::MAX);
        let mut all_candidates = Vec::new();

        for hub in hubs
            .into_iter()
            .filter(|hub| hub_is_requested(&hub.hub_id, &requested_hub_ids))
        {
            let books = self
                .market
                .latest_station_order_books(
                    hub.region_id,
                    hub.station_id,
                    &request.language,
                    fetch_limit,
                )
                .await?;
            let mut hub_candidates: Vec<_> = books
                .into_iter()
                .filter_map(|book| selection_candidate_from_order_book(book, &hub, &fee))
                .collect();
            sort_selection_candidates(&mut hub_candidates);
            hub_candidates.truncate(limit_per_hub);
            all_candidates.extend(hub_candidates);
        }

        Ok(all_candidates)
    }
}

fn hub_is_requested(hub_id: &str, requested_hub_ids: &[String]) -> bool {
    requested_hub_ids.is_empty()
        || requested_hub_ids
            .iter()
            .any(|requested| requested == hub_id)
}

fn sort_selection_candidates(candidates: &mut [SelectionCandidateView]) {
    candidates.sort_by(|left, right| {
        right
            .attention_score
            .cmp(&left.attention_score)
            .then_with(|| right.confidence_score.cmp(&left.confidence_score))
            .then_with(|| left.item_name.cmp(&right.item_name))
    });
}

fn selection_candidate_limit(limit: i64) -> Option<usize> {
    if limit <= 0 {
        None
    } else {
        Some((limit as usize).min(MAX_SELECTION_CANDIDATE_LIMIT))
    }
}

fn selection_candidate_from_order_book(
    book: StationOrderBook,
    hub: &TradeHub,
    fee: &FeeProfile,
) -> Option<SelectionCandidateView> {
    let best_bid = Decimal::from_f64(book.best_bid)?;
    let best_ask = Decimal::from_f64(book.best_ask)?;
    let top_buy_depth = u64::try_from(book.top_buy_depth).ok()?;
    let top_sell_depth = u64::try_from(book.top_sell_depth).ok()?;
    let visible_volume = u64::try_from(book.visible_volume).ok()?;
    let summary = OrderBookSummary {
        type_id: book.type_id,
        item_name: book.display_name,
        best_bid,
        best_ask,
        daily_volume: visible_volume,
        top_buy_depth,
        top_sell_depth,
        last_synced_at: book.last_synced_at.clone(),
    };
    let analysis = build_selection_candidate(&summary, fee);
    if analysis.net_profit <= Decimal::ZERO {
        return None;
    }
    Some(SelectionCandidateView::from_analysis_for_hub(
        analysis,
        SelectionCandidateHubView {
            hub_id: hub.hub_id.clone(),
            hub_name: hub.display_name.clone(),
            region_id: hub.region_id,
            station_id: hub.station_id,
            last_synced_at: book.last_synced_at,
        },
    ))
}
