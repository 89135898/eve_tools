use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncStatus {
    pub public_market_sync: String,
    pub authenticated_order_sync: String,
}

pub fn fixture_sync_status() -> SyncStatus {
    SyncStatus {
        public_market_sync: "fixture-ready".to_string(),
        authenticated_order_sync: "not-authorized".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_reports_separate_public_and_private_sync_status() {
        let status = fixture_sync_status();
        assert_eq!(status.public_market_sync, "fixture-ready");
        assert_eq!(status.authenticated_order_sync, "not-authorized");
    }
}
