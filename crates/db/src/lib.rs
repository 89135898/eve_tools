pub mod catalog;
pub mod market;
pub mod schema;

pub use catalog::{
    CatalogDbError, CatalogImportProgress, CatalogImportTable, CatalogRepository, CatalogStatus,
    ImportCatalogInput, InventoryTypeView,
};
pub use market::{
    MarketDbError, MarketOrderSnapshot, MarketOrderSnapshotInput, MarketRepository,
    StationOrderBook, TradeHub,
};
pub use schema::{connect_pool, migrate_catalog_schema};

pub fn storage_mode() -> &'static str {
    "supabase-postgres"
}
