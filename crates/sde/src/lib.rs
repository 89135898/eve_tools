pub mod archive;
pub mod client;
pub mod models;
pub mod parser;

pub use archive::{read_catalog_archive_from_bytes, CatalogArchive, SdeArchiveError};
pub use client::{SdeClient, SdeClientError, SdeLatestMetadata};
pub use models::{
    CatalogCategory, CatalogGroup, CatalogLocalization, CatalogMarketGroup, CatalogType,
    SdeMetadata,
};
pub use parser::{
    parse_category_line, parse_group_line, parse_market_group_line, parse_sde_metadata_line,
    parse_type_line, SdeParseError,
};
