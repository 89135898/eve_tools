use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, thiserror::Error)]
pub enum PublicMarketSyncError {
    #[error("market database error: {0}")]
    MarketDb(#[from] evetools_db::MarketDbError),
    #[error("public ESI error: {0}")]
    Esi(#[from] evetools_esi::EsiError),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncStatus {
    pub public_market_sync: String,
    pub authenticated_order_sync: String,
    pub data_source: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TradeHubConfig {
    pub hub_id: &'static str,
    pub display_name: &'static str,
    pub region_id: i32,
    pub system_id: i32,
    pub station_id: i64,
    pub sort_order: i32,
}

pub fn default_trade_hubs() -> Vec<TradeHubConfig> {
    vec![
        TradeHubConfig {
            hub_id: "jita",
            display_name: "Jita",
            region_id: 10000002,
            system_id: 30000142,
            station_id: 60003760,
            sort_order: 10,
        },
        TradeHubConfig {
            hub_id: "amarr",
            display_name: "Amarr",
            region_id: 10000043,
            system_id: 30002187,
            station_id: 60008494,
            sort_order: 20,
        },
        TradeHubConfig {
            hub_id: "dodixie",
            display_name: "Dodixie",
            region_id: 10000032,
            system_id: 30002659,
            station_id: 60011866,
            sort_order: 30,
        },
        TradeHubConfig {
            hub_id: "rens",
            display_name: "Rens",
            region_id: 10000030,
            system_id: 30002510,
            station_id: 60004588,
            sort_order: 40,
        },
        TradeHubConfig {
            hub_id: "hek",
            display_name: "Hek",
            region_id: 10000042,
            system_id: 30002053,
            station_id: 60005686,
            sort_order: 50,
        },
    ]
}

pub fn default_trade_hubs_as_db_records() -> Vec<evetools_db::TradeHub> {
    trade_hub_configs_as_db_records(&default_trade_hubs())
}

pub fn trade_hub_configs_as_db_records(hubs: &[TradeHubConfig]) -> Vec<evetools_db::TradeHub> {
    hubs.iter()
        .map(|hub| evetools_db::TradeHub {
            hub_id: hub.hub_id.to_string(),
            display_name: hub.display_name.to_string(),
            region_id: hub.region_id,
            system_id: hub.system_id,
            station_id: hub.station_id,
            enabled: true,
            sort_order: hub.sort_order,
        })
        .collect()
}

pub async fn sync_public_market_region_orders(
    repository: &evetools_db::MarketRepository,
    client: &evetools_esi::EsiClient,
    region_id: i32,
    hubs: &[TradeHubConfig],
) -> Result<i64, PublicMarketSyncError> {
    repository
        .upsert_trade_hubs(&trade_hub_configs_as_db_records(hubs))
        .await?;
    let sync_run_id = repository.start_sync_run(region_id, "public-esi").await?;

    let orders = match client
        .region_market_orders(region_id, evetools_esi::EsiOrderType::All)
        .await
    {
        Ok(orders) => orders,
        Err(error) => {
            let _ = repository
                .fail_sync_run(sync_run_id, &error.to_string())
                .await;
            return Err(error.into());
        }
    };

    let snapshots = market_order_snapshots_for_hubs(sync_run_id, region_id, &orders, hubs);
    if let Err(error) = repository
        .replace_order_snapshots(sync_run_id, &snapshots)
        .await
    {
        let _ = repository
            .fail_sync_run(sync_run_id, &error.to_string())
            .await;
        return Err(error.into());
    }
    repository
        .complete_sync_run(sync_run_id, 0, snapshots.len() as i64)
        .await?;
    Ok(sync_run_id)
}

pub fn market_order_snapshots_for_hubs(
    sync_run_id: i64,
    region_id: i32,
    orders: &[evetools_esi::EsiMarketOrder],
    hubs: &[TradeHubConfig],
) -> Vec<evetools_db::MarketOrderSnapshotInput> {
    let hub_station_ids: HashSet<i64> = hubs
        .iter()
        .filter(|hub| hub.region_id == region_id)
        .map(|hub| hub.station_id)
        .collect();

    orders
        .iter()
        .filter(|order| hub_station_ids.contains(&order.location_id))
        .map(|order| evetools_db::MarketOrderSnapshotInput {
            sync_run_id,
            region_id,
            station_id: order.location_id,
            type_id: order.type_id,
            order_id: order.order_id,
            is_buy_order: order.is_buy_order,
            price: order.price,
            volume_remain: i64::from(order.volume_remain),
            volume_total: i64::from(order.volume_total),
            issued: order.issued.clone(),
            duration: order.duration,
            min_volume: order.min_volume,
            order_range: order.range.clone(),
            system_id: order.system_id,
        })
        .collect()
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

    #[test]
    fn default_trade_hubs_include_major_npc_stations() {
        let hubs = default_trade_hubs();
        let hub_ids: Vec<_> = hubs.iter().map(|hub| hub.hub_id).collect();

        assert_eq!(hubs.len(), 5);
        assert_eq!(hub_ids, vec!["jita", "amarr", "dodixie", "rens", "hek"]);
        assert_eq!(hubs[0].region_id, 10000002);
        assert_eq!(hubs[0].station_id, 60003760);
    }

    #[test]
    fn market_order_snapshots_keep_only_configured_hub_stations() {
        let orders = vec![
            evetools_esi::EsiMarketOrder {
                duration: 90,
                is_buy_order: true,
                issued: "2026-05-25T11:45:00Z".to_string(),
                location_id: 60003760,
                min_volume: 1,
                order_id: 7_000_000_001,
                price: 5.01,
                range: "station".to_string(),
                system_id: 30000142,
                type_id: 34,
                volume_remain: 500_000,
                volume_total: 1_000_000,
            },
            evetools_esi::EsiMarketOrder {
                duration: 90,
                is_buy_order: false,
                issued: "2026-05-25T11:46:00Z".to_string(),
                location_id: 60000000,
                min_volume: 1,
                order_id: 7_000_000_002,
                price: 5.49,
                range: "station".to_string(),
                system_id: 30000142,
                type_id: 34,
                volume_remain: 620_000,
                volume_total: 800_000,
            },
        ];

        let snapshots =
            market_order_snapshots_for_hubs(42, 10000002, &orders, &default_trade_hubs());

        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].station_id, 60003760);
        assert_eq!(snapshots[0].order_id, 7_000_000_001);
    }
}
