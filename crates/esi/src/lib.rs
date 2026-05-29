pub mod client;
pub mod models;

use thiserror::Error;

pub use client::EsiClient;
pub use models::{
    EsiCharacterIdentity, EsiCharacterOrder, EsiMarketHistoryDay, EsiMarketOrder, EsiOrderType,
    EsiTokenResponse, EsiTypeInfo, ResolvedInventoryType, UniverseIdEntry, UniverseIdsResponse,
};

#[derive(Debug, Error)]
pub enum EsiError {
    #[error("ESI HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("ESI response could not be decoded: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("Item not found")]
    ItemNotFound,
    #[error("ESI returned status {status}: {body}")]
    Status { status: u16, body: String },
    #[error("Invalid inventory type id: {0}")]
    InvalidTypeId(String),
}

pub fn client_mode() -> &'static str {
    "live"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esi_crate_reports_live_client_mode() {
        assert_eq!(client_mode(), "live");
        assert_eq!(EsiError::ItemNotFound.to_string(), "Item not found");
    }
}
