pub mod auth;
pub mod catalog;
pub mod market;
pub mod schema;

pub use auth::{
    AuthDbError, AuthRepository, AuthorizedCharacter, CharacterAuthToken, CharacterOrderSnapshot,
    CharacterOrderSnapshotInput, CharacterOrderSyncSummary,
};
pub use catalog::{
    CatalogDbError, CatalogImportProgress, CatalogImportTable, CatalogRepository, CatalogStatus,
    ImportCatalogInput, InventoryTypeView,
};
pub use market::{
    MarketDbError, MarketOrderSnapshot, MarketOrderSnapshotInput, MarketRepository,
    MarketSyncHealthReport, MarketSyncHealthStatus, MarketSyncStartResult, MarketSyncStartStatus,
    StationOrderBook, TradeHub, TradeHubSyncHealth,
};
pub use schema::{connect_pool, migrate_catalog_schema};

pub fn storage_mode() -> &'static str {
    "supabase-postgres"
}
