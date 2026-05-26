use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncStatus {
    pub public_market_sync: String,
    pub authenticated_order_sync: String,
    pub data_source: String,
}

pub fn fixture_sync_status() -> SyncStatus {
    SyncStatus {
        public_market_sync: "fixture-ready".to_string(),
        authenticated_order_sync: "not-authorized".to_string(),
        data_source: "fixture".to_string(),
    }
}

pub fn live_sync_status() -> SyncStatus {
    SyncStatus {
        public_market_sync: "live-ready".to_string(),
        authenticated_order_sync: "not-authorized".to_string(),
        data_source: "live".to_string(),
    }
}

pub fn fixture_fallback_sync_status() -> SyncStatus {
    SyncStatus {
        public_market_sync: "fixture-fallback".to_string(),
        authenticated_order_sync: "not-authorized".to_string(),
        data_source: "fixture".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_reports_separate_public_private_and_source_status() {
        let fixture = fixture_sync_status();
        assert_eq!(fixture.public_market_sync, "fixture-ready");
        assert_eq!(fixture.authenticated_order_sync, "not-authorized");
        assert_eq!(fixture.data_source, "fixture");

        let live = live_sync_status();
        assert_eq!(live.public_market_sync, "live-ready");
        assert_eq!(live.data_source, "live");

        let fallback = fixture_fallback_sync_status();
        assert_eq!(fallback.public_market_sync, "fixture-fallback");
        assert_eq!(fallback.data_source, "fixture");
    }
}
