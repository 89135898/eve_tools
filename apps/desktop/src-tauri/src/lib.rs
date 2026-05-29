use evetools_api::{
    CatalogStatusView as CatalogStatus, InventoryTypeApiView as InventoryTypeView,
    SelectionCandidatesRequest, TradeHubView,
};
use evetools_domain::fixtures::{
    fixture_market_lookup, fixture_order_monitor, fixture_selection_candidates,
};
use evetools_domain::{MarketLookupView, OrderMonitorView, SelectionCandidateView};
use evetools_worker::{
    default_trade_hubs_as_db_records, fixture_fallback_sync_status, fixture_sync_status,
    live_sync_status, SyncStatus,
};
use serde::{de::DeserializeOwned, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::OnceCell;

const DEFAULT_SELECTION_LIMIT: i64 = 25;
const API_BASE_URL_ENV: &str = "EVETOOLS_API_BASE_URL";

static PUBLIC_MARKET_USED_FALLBACK: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug)]
enum MarketSource {
    Fixture,
    Live,
}

impl MarketSource {
    fn from_env() -> Self {
        match std::env::var("EVETOOLS_MARKET_SOURCE") {
            Ok(value) if value.eq_ignore_ascii_case("fixture") => Self::Fixture,
            _ => Self::Live,
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
async fn lookup_market_price(
    state: tauri::State<'_, ReadApiState>,
    query: String,
    language: String,
) -> Result<MarketLookupView, String> {
    lookup_market_price_with_state(&state, query, language, MarketSource::from_env()).await
}

async fn lookup_market_price_with_state(
    state: &ReadApiState,
    query: String,
    language: String,
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
        MarketSource::Live => {
            match lookup_market_price_from_hosted(state, trimmed, &language).await {
                Ok(Some(view)) => {
                    mark_public_market_fallback(false);
                    Ok(view)
                }
                Ok(None) | Err(_) => {
                    mark_public_market_fallback(true);
                    Ok(fixture_market_lookup(trimmed))
                }
            }
        }
    }
}

async fn lookup_market_price_from_hosted(
    state: &ReadApiState,
    query: &str,
    language: &str,
) -> Result<Option<MarketLookupView>, String> {
    state
        .get()
        .await?
        .lookup_market_price(query, language)
        .await
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
        MarketSource::Live => {
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
        Ok(source) => source
            .list_trade_hubs()
            .await
            .or_else(|_| Ok(default_trade_hubs_as_db_records())),
        Err(_) => Ok(default_trade_hubs_as_db_records()),
    }
}

#[derive(Default)]
struct ReadApiState {
    source: OnceCell<Arc<HostedReadApiClient>>,
}

impl ReadApiState {
    async fn get(&self) -> Result<Arc<HostedReadApiClient>, String> {
        self.source
            .get_or_try_init(|| async {
                Ok(Arc::new(HostedReadApiClient::new(
                    ReadApiSourceConfig::from_env()?.base_url,
                )))
            })
            .await
            .map(Arc::clone)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReadApiSourceConfig {
    base_url: String,
}

impl ReadApiSourceConfig {
    fn from_env() -> Result<Self, String> {
        Self::from_env_reader(|name| std::env::var(name).ok())
    }

    fn from_env_reader<F>(mut env: F) -> Result<Self, String>
    where
        F: FnMut(&str) -> Option<String>,
    {
        if let Some(base_url) = env(API_BASE_URL_ENV)
            .map(|value| value.trim().trim_end_matches('/').to_string())
            .filter(|value| !value.is_empty())
        {
            return Ok(Self { base_url });
        }

        Err("EVETOOLS_API_BASE_URL is required".to_string())
    }
}

#[derive(Clone)]
struct HostedReadApiClient {
    base_url: String,
    http: reqwest::Client,
}

impl HostedReadApiClient {
    fn new(base_url: String) -> Self {
        Self {
            base_url,
            http: reqwest::Client::new(),
        }
    }

    async fn catalog_status(&self) -> Result<CatalogStatus, String> {
        self.get_json("/catalog/status", &NoQuery).await
    }

    async fn get_inventory_type(
        &self,
        type_id: i32,
        language: &str,
    ) -> Result<Option<InventoryTypeView>, String> {
        self.get_json(
            &format!("/inventory-types/{type_id}"),
            &LanguageHttpQuery { language },
        )
        .await
    }

    async fn search_inventory_types(
        &self,
        query: &str,
        language: &str,
        limit: i64,
    ) -> Result<Vec<InventoryTypeView>, String> {
        self.get_json(
            "/inventory-types/search",
            &InventoryTypeSearchHttpQuery {
                query,
                language,
                limit,
            },
        )
        .await
    }

    async fn lookup_market_price(
        &self,
        query: &str,
        language: &str,
    ) -> Result<Option<MarketLookupView>, String> {
        self.get_json(
            "/market-lookup",
            &MarketLookupHttpQuery {
                query,
                language,
                hub_id: "jita",
            },
        )
        .await
    }

    async fn list_trade_hubs(&self) -> Result<Vec<TradeHubView>, String> {
        self.get_json("/trade-hubs", &NoQuery).await
    }

    async fn selection_candidates(
        &self,
        request: SelectionCandidatesRequest,
    ) -> Result<Vec<SelectionCandidateView>, String> {
        let hub_ids = request.hub_ids.join(",");
        self.get_json(
            "/selection-candidates",
            &SelectionCandidatesHttpQuery {
                hub_ids: &hub_ids,
                language: &request.language,
                limit_per_hub: request.limit_per_hub,
            },
        )
        .await
    }

    async fn get_json<T, Q>(&self, path: &str, query: &Q) -> Result<T, String>
    where
        T: DeserializeOwned,
        Q: Serialize + ?Sized,
    {
        self.http
            .get(format!("{}{}", self.base_url, path))
            .query(query)
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .json::<T>()
            .await
            .map_err(|error| error.to_string())
    }
}

#[derive(Serialize)]
struct NoQuery;

#[derive(Serialize)]
struct LanguageHttpQuery<'a> {
    language: &'a str,
}

#[derive(Serialize)]
struct InventoryTypeSearchHttpQuery<'a> {
    query: &'a str,
    language: &'a str,
    limit: i64,
}

#[derive(Serialize)]
struct MarketLookupHttpQuery<'a> {
    query: &'a str,
    language: &'a str,
    hub_id: &'a str,
}

#[derive(Serialize)]
struct SelectionCandidatesHttpQuery<'a> {
    hub_ids: &'a str,
    language: &'a str,
    limit_per_hub: i64,
}

#[tauri::command]
async fn get_sde_catalog_status(
    state: tauri::State<'_, ReadApiState>,
) -> Result<CatalogStatus, String> {
    state.get().await?.catalog_status().await
}

#[tauri::command]
async fn import_sde_catalog_latest(
    _state: tauri::State<'_, ReadApiState>,
) -> Result<CatalogStatus, String> {
    Err(
        "SDE catalog import is an admin operation; run import-sde-latest or a hosted job instead"
            .to_string(),
    )
}

#[tauri::command]
async fn search_inventory_types(
    state: tauri::State<'_, ReadApiState>,
    query: String,
    language: String,
    limit: i64,
) -> Result<Vec<InventoryTypeView>, String> {
    state
        .get()
        .await?
        .search_inventory_types(&query, &language, limit)
        .await
}

#[tauri::command]
async fn get_inventory_type(
    state: tauri::State<'_, ReadApiState>,
    type_id: i32,
    language: String,
) -> Result<Option<InventoryTypeView>, String> {
    state
        .get()
        .await?
        .get_inventory_type(type_id, &language)
        .await
}

pub fn run() {
    tauri::Builder::default()
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
        let state = ReadApiState::default();
        let result = lookup_market_price_with_state(
            &state,
            "   ".to_string(),
            "en-US".to_string(),
            MarketSource::Fixture,
        )
        .await;
        assert_eq!(result.unwrap_err(), "Item query is required");
    }

    #[tokio::test]
    async fn fixture_source_returns_mvp_views_without_network() {
        let state = ReadApiState::default();
        assert_eq!(
            lookup_market_price_with_state(
                &state,
                "Tritanium".to_string(),
                "en-US".to_string(),
                MarketSource::Fixture,
            )
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
            get_sync_status_with_source(MarketSource::Live)
                .unwrap()
                .public_market_sync,
            "live-ready"
        );

        mark_public_market_fallback(true);
        assert_eq!(
            get_sync_status_with_source(MarketSource::Live)
                .unwrap()
                .public_market_sync,
            "fixture-fallback"
        );
        mark_public_market_fallback(false);
    }

    #[test]
    fn read_api_config_uses_hosted_api_base_url() {
        let config = ReadApiSourceConfig::from_env_reader(|name| match name {
            "EVETOOLS_API_BASE_URL" => Some(" http://127.0.0.1:8080/ ".to_string()),
            _ => None,
        })
        .unwrap();

        assert_eq!(
            config,
            ReadApiSourceConfig {
                base_url: "http://127.0.0.1:8080".to_string()
            }
        );
    }

    #[test]
    fn read_api_config_requires_hosted_api_base_url() {
        let error = ReadApiSourceConfig::from_env_reader(|_| None).unwrap_err();

        assert_eq!(error, "EVETOOLS_API_BASE_URL is required");
    }

    #[tokio::test]
    async fn hosted_lookup_errors_fall_back_to_fixture_data() {
        let state = ReadApiState::default();
        let result = lookup_market_price_with_state(
            &state,
            "Tritanium".to_string(),
            "zh-CN".to_string(),
            MarketSource::Live,
        )
        .await
        .unwrap();

        assert_eq!(result.type_id, 34);
        assert_eq!(result.item_name, "Tritanium");
    }
}
