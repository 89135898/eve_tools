use chrono::Utc;
use evetools_api::{EveToolsReadApi, SelectionCandidatesRequest, TradeHubView};
use evetools_catalog::{CatalogConfig, CatalogService};
use evetools_db::{CatalogStatus, InventoryTypeView};
use evetools_domain::fixtures::{
    fixture_market_lookup, fixture_order_monitor, fixture_selection_candidates,
};
use evetools_domain::{
    classify_price_trend, summarize_jita_market, MarketLookupView, OrderMonitorView,
    PublicMarketHistoryDay, PublicMarketOrder, SelectionCandidateView, THE_FORGE_REGION_ID,
};
use evetools_esi::{EsiClient, EsiError, EsiMarketHistoryDay, EsiMarketOrder, EsiOrderType};
use evetools_worker::{
    default_trade_hubs_as_db_records, fixture_fallback_sync_status, fixture_sync_status,
    live_sync_status, SyncStatus,
};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::OnceCell;

const DEFAULT_SELECTION_LIMIT: i64 = 25;

static PUBLIC_MARKET_USED_FALLBACK: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug)]
enum MarketSource {
    Fixture,
    Live(EsiClient),
}

impl MarketSource {
    fn from_env() -> Self {
        match std::env::var("EVETOOLS_MARKET_SOURCE") {
            Ok(value) if value.eq_ignore_ascii_case("fixture") => Self::Fixture,
            _ => Self::Live(EsiClient::tranquility()),
        }
    }

    fn is_fixture(&self) -> bool {
        matches!(self, Self::Fixture)
    }
}

fn mark_public_market_fallback(used_fallback: bool) {
    PUBLIC_MARKET_USED_FALLBACK.store(used_fallback, Ordering::Relaxed);
}

fn public_market_used_fallback() -> bool {
    PUBLIC_MARKET_USED_FALLBACK.load(Ordering::Relaxed)
}

#[tauri::command]
async fn lookup_market_price(query: String) -> Result<MarketLookupView, String> {
    lookup_market_price_with_source(query, MarketSource::from_env()).await
}

async fn lookup_market_price_with_source(
    query: String,
    source: MarketSource,
) -> Result<MarketLookupView, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err("Item query is required".to_string());
    }

    match source {
        MarketSource::Fixture => {
            mark_public_market_fallback(false);
            Ok(fixture_market_lookup(trimmed))
        }
        MarketSource::Live(client) => match lookup_market_price_live(trimmed, &client).await {
            Ok(view) => {
                mark_public_market_fallback(false);
                Ok(view)
            }
            Err(EsiError::ItemNotFound) => {
                mark_public_market_fallback(false);
                Err("Item not found".to_string())
            }
            Err(_) => {
                mark_public_market_fallback(true);
                Ok(fixture_market_lookup(trimmed))
            }
        },
    }
}

async fn lookup_market_price_live(
    query: &str,
    client: &EsiClient,
) -> Result<MarketLookupView, EsiError> {
    let resolved = client.resolve_inventory_type(query).await?;
    let orders = client
        .market_orders(THE_FORGE_REGION_ID, resolved.type_id, EsiOrderType::All)
        .await?;
    let history = client
        .market_history(THE_FORGE_REGION_ID, resolved.type_id)
        .await?;

    let domain_orders = to_domain_orders(&orders);
    let domain_history = to_domain_history(&history);
    let summary = summarize_jita_market(
        resolved.type_id,
        resolved.name,
        &domain_orders,
        &domain_history,
        Utc::now().to_rfc3339(),
    );
    let trend = classify_price_trend(&domain_history);

    Ok(MarketLookupView::from_summary(summary, trend))
}

#[tauri::command]
async fn list_selection_candidates(
    state: tauri::State<'_, ReadApiState>,
    language: String,
    hub_ids: Vec<String>,
) -> Result<Vec<SelectionCandidateView>, String> {
    if MarketSource::from_env().is_fixture() {
        return list_selection_candidates_with_source(MarketSource::Fixture).await;
    }

    match list_selection_candidates_from_snapshots(&state, language, hub_ids).await {
        Ok(candidates) if !candidates.is_empty() => {
            mark_public_market_fallback(false);
            Ok(candidates)
        }
        Ok(_) | Err(_) => {
            mark_public_market_fallback(true);
            Ok(fixture_selection_candidates())
        }
    }
}

async fn list_selection_candidates_with_source(
    source: MarketSource,
) -> Result<Vec<SelectionCandidateView>, String> {
    match source {
        MarketSource::Fixture => {
            mark_public_market_fallback(false);
            Ok(fixture_selection_candidates())
        }
        MarketSource::Live(_) => {
            mark_public_market_fallback(true);
            Ok(fixture_selection_candidates())
        }
    }
}

async fn list_selection_candidates_from_snapshots(
    state: &ReadApiState,
    language: String,
    hub_ids: Vec<String>,
) -> Result<Vec<SelectionCandidateView>, String> {
    state
        .get()
        .await?
        .selection_candidates(SelectionCandidatesRequest {
            hub_ids,
            language,
            limit_per_hub: DEFAULT_SELECTION_LIMIT,
        })
        .await
        .map_err(|error| error.to_string())
}

fn to_domain_orders(orders: &[EsiMarketOrder]) -> Vec<PublicMarketOrder> {
    orders
        .iter()
        .filter_map(|order| {
            Some(PublicMarketOrder {
                type_id: order.type_id,
                location_id: order.location_id,
                is_buy_order: order.is_buy_order,
                price: Decimal::from_f64(order.price)?,
                volume_remain: u64::try_from(order.volume_remain).ok()?,
            })
        })
        .collect()
}

fn to_domain_history(history: &[EsiMarketHistoryDay]) -> Vec<PublicMarketHistoryDay> {
    history
        .iter()
        .filter_map(|day| {
            Some(PublicMarketHistoryDay {
                average: Decimal::from_f64(day.average)?,
                date: day.date.clone(),
                volume: u64::try_from(day.volume).ok()?,
            })
        })
        .collect()
}

#[tauri::command]
fn list_order_monitor_items() -> Result<Vec<OrderMonitorView>, String> {
    Ok(fixture_order_monitor())
}

#[tauri::command]
fn get_sync_status() -> Result<SyncStatus, String> {
    get_sync_status_with_source(MarketSource::from_env())
}

fn get_sync_status_with_source(source: MarketSource) -> Result<SyncStatus, String> {
    if source.is_fixture() {
        Ok(fixture_sync_status())
    } else if public_market_used_fallback() {
        Ok(fixture_fallback_sync_status())
    } else {
        Ok(live_sync_status())
    }
}

#[tauri::command]
async fn list_trade_hubs(
    state: tauri::State<'_, ReadApiState>,
) -> Result<Vec<TradeHubView>, String> {
    if MarketSource::from_env().is_fixture() {
        return Ok(default_trade_hubs_as_db_records());
    }

    match state.get().await {
        Ok(api) => api
            .list_trade_hubs()
            .await
            .map_err(|error| error.to_string())
            .or_else(|_| Ok(default_trade_hubs_as_db_records())),
        Err(_) => Ok(default_trade_hubs_as_db_records()),
    }
}

#[derive(Default)]
struct CatalogServiceState {
    service: OnceCell<Arc<CatalogService>>,
}

impl CatalogServiceState {
    async fn get(&self) -> Result<Arc<CatalogService>, String> {
        self.service
            .get_or_try_init(|| async {
                let config = CatalogConfig::from_env().map_err(|error| error.to_string())?;
                CatalogService::connect(config)
                    .await
                    .map(Arc::new)
                    .map_err(|error| error.to_string())
            })
            .await
            .map(Arc::clone)
    }
}

#[derive(Default)]
struct ReadApiState {
    api: OnceCell<Arc<EveToolsReadApi>>,
}

impl ReadApiState {
    async fn get(&self) -> Result<Arc<EveToolsReadApi>, String> {
        self.api
            .get_or_try_init(|| async {
                let database_url = std::env::var("EVETOOLS_DATABASE_URL")
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if database_url.is_empty() {
                    return Err("EVETOOLS_DATABASE_URL is required".to_string());
                }
                EveToolsReadApi::connect(&database_url)
                    .await
                    .map(Arc::new)
                    .map_err(|error| error.to_string())
            })
            .await
            .map(Arc::clone)
    }
}

#[tauri::command]
async fn get_sde_catalog_status(
    state: tauri::State<'_, CatalogServiceState>,
) -> Result<CatalogStatus, String> {
    state
        .get()
        .await?
        .status()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn import_sde_catalog_latest(
    state: tauri::State<'_, CatalogServiceState>,
) -> Result<CatalogStatus, String> {
    state
        .get()
        .await?
        .import_latest()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn search_inventory_types(
    state: tauri::State<'_, CatalogServiceState>,
    query: String,
    language: String,
    limit: i64,
) -> Result<Vec<InventoryTypeView>, String> {
    state
        .get()
        .await?
        .search_inventory_types(&query, &language, limit)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_inventory_type(
    state: tauri::State<'_, CatalogServiceState>,
    type_id: i32,
    language: String,
) -> Result<Option<InventoryTypeView>, String> {
    state
        .get()
        .await?
        .get_inventory_type(type_id, &language)
        .await
        .map_err(|error| error.to_string())
}

pub fn run() {
    tauri::Builder::default()
        .manage(CatalogServiceState::default())
        .manage(ReadApiState::default())
        .invoke_handler(tauri::generate_handler![
            lookup_market_price,
            list_selection_candidates,
            list_trade_hubs,
            list_order_monitor_items,
            get_sync_status,
            get_sde_catalog_status,
            import_sde_catalog_latest,
            search_inventory_types,
            get_inventory_type
        ])
        .run(tauri::generate_context!())
        .expect("failed to run EveTools desktop application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lookup_rejects_empty_query() {
        let result =
            lookup_market_price_with_source("   ".to_string(), MarketSource::Fixture).await;
        assert_eq!(result.unwrap_err(), "Item query is required");
    }

    #[tokio::test]
    async fn fixture_source_returns_mvp_views_without_network() {
        assert_eq!(
            lookup_market_price_with_source("Tritanium".to_string(), MarketSource::Fixture)
                .await
                .unwrap()
                .item_name,
            "Tritanium"
        );
        assert_eq!(
            list_selection_candidates_with_source(MarketSource::Fixture)
                .await
                .unwrap()
                .len(),
            2
        );
        assert_eq!(list_order_monitor_items().unwrap().len(), 2);
    }

    #[test]
    fn worker_status_reports_live_fixture_and_fallback_sources() {
        assert_eq!(
            evetools_worker::live_sync_status().public_market_sync,
            "live-ready"
        );
        assert_eq!(evetools_worker::live_sync_status().data_source, "live");
        assert_eq!(
            evetools_worker::fixture_fallback_sync_status().public_market_sync,
            "fixture-fallback"
        );
        assert_eq!(
            evetools_worker::fixture_fallback_sync_status().data_source,
            "fixture"
        );
    }

    #[test]
    fn sync_status_uses_last_public_market_fallback_signal() {
        mark_public_market_fallback(false);
        assert_eq!(
            get_sync_status_with_source(MarketSource::Fixture)
                .unwrap()
                .public_market_sync,
            "fixture-ready"
        );
        assert_eq!(
            get_sync_status_with_source(MarketSource::Live(EsiClient::new("http://127.0.0.1:9")))
                .unwrap()
                .public_market_sync,
            "live-ready"
        );

        mark_public_market_fallback(true);
        assert_eq!(
            get_sync_status_with_source(MarketSource::Live(EsiClient::new("http://127.0.0.1:9")))
                .unwrap()
                .public_market_sync,
            "fixture-fallback"
        );
        mark_public_market_fallback(false);
    }

    #[tokio::test]
    async fn catalog_service_state_reports_missing_database_url() {
        std::env::remove_var("EVETOOLS_DATABASE_URL");

        let state = CatalogServiceState::default();
        let error = match state.get().await {
            Ok(_) => panic!("expected missing database url error"),
            Err(error) => error,
        };

        assert_eq!(error, "EVETOOLS_DATABASE_URL is required");
        assert!(state.service.get().is_none());
    }
}
