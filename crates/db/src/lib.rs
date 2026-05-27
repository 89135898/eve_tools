pub mod catalog;
pub mod schema;

pub use catalog::{
    CatalogDbError, CatalogImportProgress, CatalogImportTable, CatalogRepository, CatalogStatus,
    ImportCatalogInput, InventoryTypeView,
};
pub use schema::{connect_pool, migrate_catalog_schema};

pub fn storage_mode() -> &'static str {
    "supabase-postgres-catalog"
}
