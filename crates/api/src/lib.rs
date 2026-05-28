use evetools_db::{
    connect_pool, migrate_catalog_schema, CatalogDbError, CatalogRepository, CatalogStatus,
    InventoryTypeView, MarketDbError, MarketOrderSnapshot, MarketRepository, TradeHub,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use thiserror::Error;

pub use evetools_db::{MarketOrderSnapshot as MarketOrderView, TradeHub as TradeHubView};

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
}
