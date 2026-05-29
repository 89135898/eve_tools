use evetools_api::{
    CatalogStatusView as CatalogStatus, InventoryTypeApiView as InventoryTypeView,
    SelectionCandidatesRequest, TradeHubView,
};
use evetools_domain::fixtures::{
    fixture_market_lookup, fixture_order_monitor, fixture_selection_candidates,
};
use evetools_domain::{MarketLookupView, OrderMonitorView, SelectionCandidateView};
use evetools_worker::{
    default_trade_hubs_as_db_records, fixture_sync_status, live_sync_status, SyncStatus,
};
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::OnceCell;

const DEFAULT_SELECTION_LIMIT: i64 = 25;
const API_BASE_URL_ENV: &str = "EVETOOLS_API_BASE_URL";
const BUILD_TIME_API_BASE_URL: Option<&str> = option_env!("EVETOOLS_API_BASE_URL");
const BACKEND_PROBE_PATHS: [&str; 3] = ["/health", "/ready", "/sync-health"];

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
        MarketSource::Fixture => Ok(fixture_market_lookup(trimmed)),
        MarketSource::Live => lookup_market_price_from_hosted(state, trimmed, &language)
            .await?
            .ok_or_else(|| format!("No market data found for {trimmed}")),
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
    list_selection_candidates_with_state(&state, language, hub_ids, MarketSource::from_env()).await
}

async fn list_selection_candidates_with_source(
    source: MarketSource,
) -> Result<Vec<SelectionCandidateView>, String> {
    match source {
        MarketSource::Fixture => Ok(fixture_selection_candidates()),
        MarketSource::Live => Err("live selection candidates require hosted API state".to_string()),
    }
}

async fn list_selection_candidates_with_state(
    state: &ReadApiState,
    language: String,
    hub_ids: Vec<String>,
    source: MarketSource,
) -> Result<Vec<SelectionCandidateView>, String> {
    match source {
        MarketSource::Fixture => list_selection_candidates_with_source(MarketSource::Fixture).await,
        MarketSource::Live => list_selection_candidates_from_snapshots(state, language, hub_ids).await,
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
    } else {
        Ok(live_sync_status())
    }
}

#[tauri::command]
async fn list_trade_hubs(
    state: tauri::State<'_, ReadApiState>,
) -> Result<Vec<TradeHubView>, String> {
    list_trade_hubs_with_state(&state, MarketSource::from_env()).await
}

async fn list_trade_hubs_with_state(
    state: &ReadApiState,
    source: MarketSource,
) -> Result<Vec<TradeHubView>, String> {
    if source.is_fixture() {
        return Ok(default_trade_hubs_as_db_records());
    }

    match state.get().await {
        Ok(source) => source.list_trade_hubs().await,
        Err(error) => Err(error),
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
        Self::from_sources(|name| std::env::var(name).ok(), BUILD_TIME_API_BASE_URL)
    }

    #[cfg(test)]
    fn from_env_reader<F>(env: F) -> Result<Self, String>
    where
        F: FnMut(&str) -> Option<String>,
    {
        Self::from_sources(env, None)
    }

    fn from_sources<F>(mut env: F, build_time_base_url: Option<&str>) -> Result<Self, String>
    where
        F: FnMut(&str) -> Option<String>,
    {
        if let Some(base_url) =
            env(API_BASE_URL_ENV).and_then(|value| normalize_base_url(&value))
        {
            return Ok(Self { base_url });
        }

        if let Some(base_url) = build_time_base_url.and_then(normalize_base_url) {
            return Ok(Self { base_url });
        }

        Err("EVETOOLS_API_BASE_URL is required".to_string())
    }
}

fn normalize_base_url(value: &str) -> Option<String> {
    let base_url = value.trim().trim_end_matches('/').to_string();
    (!base_url.is_empty()).then_some(base_url)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct BackendProbeView {
    path: String,
    status: String,
    http_status: Option<u16>,
    message: Option<String>,
}

impl BackendProbeView {
    fn ok(path: &str, http_status: u16) -> Self {
        Self {
            path: path.to_string(),
            status: "ok".to_string(),
            http_status: Some(http_status),
            message: None,
        }
    }

    fn error(path: &str, http_status: Option<u16>, message: String) -> Self {
        Self {
            path: path.to_string(),
            status: "error".to_string(),
            http_status,
            message: Some(message),
        }
    }

    fn not_configured(path: &str, message: &str) -> Self {
        Self {
            path: path.to_string(),
            status: "not-configured".to_string(),
            http_status: None,
            message: Some(message.to_string()),
        }
    }

    fn is_ok(&self) -> bool {
        self.status == "ok"
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct BackendConnectionStatusView {
    configured: bool,
    base_url: Option<String>,
    overall_status: String,
    probes: Vec<BackendProbeView>,
}

impl BackendConnectionStatusView {
    fn not_configured(message: String) -> Self {
        Self {
            configured: false,
            base_url: None,
            overall_status: "not-configured".to_string(),
            probes: BACKEND_PROBE_PATHS
                .iter()
                .map(|path| BackendProbeView::not_configured(path, &message))
                .collect(),
        }
    }

    fn from_probes(base_url: String, probes: Vec<BackendProbeView>) -> Self {
        let health_ok = probes
            .iter()
            .find(|probe| probe.path == "/health")
            .is_some_and(BackendProbeView::is_ok);
        let all_ok = probes.iter().all(BackendProbeView::is_ok);
        let overall_status = if all_ok {
            "ready"
        } else if health_ok {
            "degraded"
        } else {
            "offline"
        }
        .to_string();

        Self {
            configured: true,
            base_url: Some(base_url),
            overall_status,
            probes,
        }
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
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("failed to build hosted API HTTP client"),
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

    async fn backend_connection_status(&self) -> BackendConnectionStatusView {
        let (health, readiness, sync_health) = tokio::join!(
            self.probe("/health"),
            self.probe("/ready"),
            self.probe("/sync-health")
        );
        let probes = vec![health, readiness, sync_health];
        BackendConnectionStatusView::from_probes(self.base_url.clone(), probes)
    }

    async fn probe(&self, path: &str) -> BackendProbeView {
        match self.http.get(format!("{}{}", self.base_url, path)).send().await {
            Ok(response) => {
                let status = response.status();
                let http_status = status.as_u16();
                if status.is_success() {
                    BackendProbeView::ok(path, http_status)
                } else {
                    let message = response
                        .text()
                        .await
                        .unwrap_or_else(|error| error.to_string());
                    BackendProbeView::error(path, Some(http_status), message)
                }
            }
            Err(error) => BackendProbeView::error(path, None, error.to_string()),
        }
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

#[tauri::command]
async fn get_backend_connection_status(
    state: tauri::State<'_, ReadApiState>,
) -> Result<BackendConnectionStatusView, String> {
    match state.get().await {
        Ok(source) => Ok(source.backend_connection_status().await),
        Err(error) => Ok(BackendConnectionStatusView::not_configured(error)),
    }
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
            get_inventory_type,
            get_backend_connection_status
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
    fn worker_status_reports_live_and_fixture_sources() {
        assert_eq!(
            evetools_worker::live_sync_status().public_market_sync,
            "live-ready"
        );
        assert_eq!(evetools_worker::live_sync_status().data_source, "live");
    }

    #[test]
    fn sync_status_depends_only_on_explicit_source() {
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

    #[test]
    fn read_api_config_uses_build_time_base_url_when_runtime_env_is_missing() {
        let config =
            ReadApiSourceConfig::from_sources(|_| None, Some(" https://api.example.com/ "))
                .unwrap();

        assert_eq!(
            config,
            ReadApiSourceConfig {
                base_url: "https://api.example.com".to_string()
            }
        );
    }

    #[test]
    fn read_api_config_runtime_env_overrides_build_time_base_url() {
        let config = ReadApiSourceConfig::from_sources(
            |name| match name {
                "EVETOOLS_API_BASE_URL" => Some(" https://runtime.example.com/ ".to_string()),
                _ => None,
            },
            Some("https://build.example.com"),
        )
        .unwrap();

        assert_eq!(
            config,
            ReadApiSourceConfig {
                base_url: "https://runtime.example.com".to_string()
            }
        );
    }

    #[test]
    fn backend_connection_status_reports_not_configured_without_base_url() {
        let status = BackendConnectionStatusView::not_configured(
            "EVETOOLS_API_BASE_URL is required".to_string(),
        );

        assert!(!status.configured);
        assert_eq!(status.overall_status, "not-configured");
        assert_eq!(
            status
                .probes
                .iter()
                .map(|probe| probe.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/health", "/ready", "/sync-health"]
        );
        assert!(
            status
                .probes
                .iter()
                .all(|probe| probe.status == "not-configured")
        );
    }

    #[test]
    fn backend_connection_status_distinguishes_ready_degraded_and_offline() {
        let ready = BackendConnectionStatusView::from_probes(
            "https://api.example.com".to_string(),
            vec![
                BackendProbeView::ok("/health", 200),
                BackendProbeView::ok("/ready", 200),
                BackendProbeView::ok("/sync-health", 200),
            ],
        );
        assert_eq!(ready.overall_status, "ready");

        let degraded = BackendConnectionStatusView::from_probes(
            "https://api.example.com".to_string(),
            vec![
                BackendProbeView::ok("/health", 200),
                BackendProbeView::ok("/ready", 200),
                BackendProbeView::error("/sync-health", Some(500), "sync check failed".to_string()),
            ],
        );
        assert_eq!(degraded.overall_status, "degraded");

        let offline = BackendConnectionStatusView::from_probes(
            "https://api.example.com".to_string(),
            vec![
                BackendProbeView::error("/health", None, "connection refused".to_string()),
                BackendProbeView::error("/ready", None, "connection refused".to_string()),
                BackendProbeView::error("/sync-health", None, "connection refused".to_string()),
            ],
        );
        assert_eq!(offline.overall_status, "offline");
    }

    #[tokio::test]
    async fn live_lookup_errors_are_reported_without_fixture_fallback() {
        let state = ReadApiState::default();
        let error = lookup_market_price_with_state(
            &state,
            "Tritanium".to_string(),
            "zh-CN".to_string(),
            MarketSource::Live,
        )
        .await
        .unwrap_err();

        assert_eq!(error, "EVETOOLS_API_BASE_URL is required");
    }

    #[tokio::test]
    async fn live_selection_errors_are_reported_without_fixture_fallback() {
        let state = ReadApiState::default();
        let error = list_selection_candidates_with_state(
            &state,
            "zh-CN".to_string(),
            vec!["jita".to_string()],
            MarketSource::Live,
        )
        .await
        .unwrap_err();

        assert_eq!(error, "EVETOOLS_API_BASE_URL is required");
    }

    #[tokio::test]
    async fn live_trade_hubs_errors_are_reported_without_fixture_fallback() {
        let state = ReadApiState::default();
        let error = list_trade_hubs_with_state(&state, MarketSource::Live)
            .await
            .unwrap_err();

        assert_eq!(error, "EVETOOLS_API_BASE_URL is required");
    }
}
