use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use evetools_api::{
    CatalogStatusView as CatalogStatus, InventoryTypeApiView as InventoryTypeView,
    SelectionCandidatesRequest, TradeHubView,
};
use evetools_db::{AuthRepository, AuthorizedCharacter, CharacterAuthToken};
use evetools_domain::fixtures::{
    fixture_market_lookup, fixture_order_monitor, fixture_selection_candidates,
};
use evetools_domain::{MarketLookupView, OrderMonitorView, SelectionCandidateView};
use evetools_worker::{
    default_trade_hubs_as_db_records, fixture_sync_status, live_sync_status,
    sync_authenticated_character_orders, AuthenticatedOrderSyncSummary, SyncStatus,
};
use rand::{distributions::Alphanumeric, Rng};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{process::Command, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::OnceCell,
};
use url::Url;

const DEFAULT_SELECTION_LIMIT: i64 = 25;
const API_BASE_URL_ENV: &str = "EVETOOLS_API_BASE_URL";
const BUILD_TIME_API_BASE_URL: Option<&str> = option_env!("EVETOOLS_API_BASE_URL");
const BACKEND_PROBE_PATHS: [&str; 3] = ["/health", "/ready", "/sync-health"];
const DATABASE_URL_ENV: &str = "EVETOOLS_DATABASE_URL";
const ESI_BASE_URL_ENV: &str = "EVETOOLS_ESI_BASE_URL";
const SSO_BASE_URL_ENV: &str = "EVETOOLS_SSO_BASE_URL";
const SSO_CLIENT_ID_ENV: &str = "EVETOOLS_SSO_CLIENT_ID";
const SSO_REDIRECT_URI_ENV: &str = "EVETOOLS_SSO_REDIRECT_URI";
const DEFAULT_SSO_BASE_URL: &str = "https://login.eveonline.com";
const REQUIRED_CHARACTER_ORDER_SCOPE: &str = "esi-markets.read_character_orders.v1";

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
        MarketSource::Live => {
            list_selection_candidates_from_snapshots(state, language, hub_ids).await
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
async fn list_order_monitor_items(
    state: tauri::State<'_, ReadApiState>,
    language: String,
) -> Result<Vec<OrderMonitorView>, String> {
    list_order_monitor_items_with_state(&state, language, MarketSource::from_env()).await
}

async fn list_order_monitor_items_with_source(
    source: MarketSource,
) -> Result<Vec<OrderMonitorView>, String> {
    if source.is_fixture() {
        Ok(fixture_order_monitor())
    } else {
        Ok(Vec::new())
    }
}

async fn list_order_monitor_items_with_state(
    state: &ReadApiState,
    language: String,
    source: MarketSource,
) -> Result<Vec<OrderMonitorView>, String> {
    if source.is_fixture() {
        return list_order_monitor_items_with_source(MarketSource::Fixture).await;
    }

    let auth = get_auth_status_from_env().await?;
    let Some(character_id) = auth.character_id else {
        return Ok(Vec::new());
    };
    state
        .get()
        .await?
        .order_monitor_items(character_id, &language)
        .await
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
        if let Some(base_url) = env(API_BASE_URL_ENV).and_then(|value| normalize_base_url(&value)) {
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct EveSsoConfig {
    client_id: String,
    redirect_uri: String,
    sso_base_url: String,
}

impl EveSsoConfig {
    fn from_env() -> Result<Self, String> {
        Self::from_sources(|name| std::env::var(name).ok())
    }

    fn from_sources<F>(mut source: F) -> Result<Self, String>
    where
        F: FnMut(&str) -> Option<String>,
    {
        let client_id = source(SSO_CLIENT_ID_ENV)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("{SSO_CLIENT_ID_ENV} is required"))?;
        let redirect_uri = source(SSO_REDIRECT_URI_ENV)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("{SSO_REDIRECT_URI_ENV} is required"))?;
        let sso_base_url = source(SSO_BASE_URL_ENV)
            .and_then(|value| normalize_base_url(&value))
            .unwrap_or_else(|| DEFAULT_SSO_BASE_URL.to_string());

        Ok(Self {
            client_id,
            redirect_uri,
            sso_base_url,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct AuthStatusView {
    status: String,
    configured: bool,
    character_id: Option<i64>,
    character_name: Option<String>,
    latest_sync_status: Option<String>,
    latest_sync_completed_at: Option<String>,
    message: Option<String>,
}

impl AuthStatusView {
    fn not_configured(message: String) -> Self {
        Self {
            status: "not-configured".to_string(),
            configured: false,
            character_id: None,
            character_name: None,
            latest_sync_status: None,
            latest_sync_completed_at: None,
            message: Some(message),
        }
    }

    fn not_authorized() -> Self {
        Self {
            status: "not-authorized".to_string(),
            configured: true,
            character_id: None,
            character_name: None,
            latest_sync_status: None,
            latest_sync_completed_at: None,
            message: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct EveAccessTokenClaims {
    sub: String,
    name: Option<String>,
    owner: Option<String>,
    scp: Option<ScopesClaim>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ScopesClaim {
    Single(String),
    Multiple(Vec<String>),
}

impl ScopesClaim {
    fn into_vec(self) -> Vec<String> {
        match self {
            Self::Single(value) => value
                .split_whitespace()
                .filter(|scope| !scope.is_empty())
                .map(ToString::to_string)
                .collect(),
            Self::Multiple(values) => values,
        }
    }
}

fn build_sso_authorization_url(
    config: &EveSsoConfig,
    state: &str,
    code_challenge: &str,
) -> Result<String, String> {
    let mut url = Url::parse(&format!("{}/v2/oauth/authorize/", config.sso_base_url))
        .map_err(|error| format!("invalid EVE SSO base URL: {error}"))?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", &config.redirect_uri)
        .append_pair("client_id", &config.client_id)
        .append_pair("scope", REQUIRED_CHARACTER_ORDER_SCOPE)
        .append_pair("state", state)
        .append_pair("code_challenge", code_challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(url.to_string())
}

fn generate_pkce_verifier() -> String {
    rand::thread_rng()
        .sample_iter(Alphanumeric)
        .take(64)
        .map(char::from)
        .collect()
}

fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn generate_oauth_state() -> String {
    rand::thread_rng()
        .sample_iter(Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

fn decode_eve_access_token_identity(
    access_token: &str,
) -> Result<evetools_esi::EsiCharacterIdentity, String> {
    let payload = access_token
        .split('.')
        .nth(1)
        .ok_or_else(|| "EVE access token is not a JWT".to_string())?;
    let decoded = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|error| format!("EVE access token payload is invalid: {error}"))?;
    let claims: EveAccessTokenClaims = serde_json::from_slice(&decoded)
        .map_err(|error| format!("EVE access token claims are invalid: {error}"))?;
    let character_id = claims
        .sub
        .strip_prefix("CHARACTER:EVE:")
        .ok_or_else(|| "EVE access token subject is not a character".to_string())?
        .parse::<i64>()
        .map_err(|error| format!("EVE character id is invalid: {error}"))?;
    let scopes = claims.scp.map(ScopesClaim::into_vec).unwrap_or_default();

    Ok(evetools_esi::EsiCharacterIdentity {
        character_id,
        character_name: claims
            .name
            .unwrap_or_else(|| format!("Character {character_id}")),
        owner_hash: claims.owner,
        scopes,
    })
}

async fn auth_repository_from_env() -> Result<AuthRepository, String> {
    let database_url = std::env::var(DATABASE_URL_ENV)
        .map(|value| value.trim().to_string())
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{DATABASE_URL_ENV} is required"))?;
    let pool = evetools_db::connect_pool(&database_url)
        .await
        .map_err(|error| format!("database connection failed: {error}"))?;
    evetools_db::migrate_catalog_schema(&pool)
        .await
        .map_err(|error| format!("database migration failed: {error}"))?;
    Ok(AuthRepository::new(pool))
}

async fn auth_status_from_repository(
    repository: &AuthRepository,
) -> Result<AuthStatusView, String> {
    let Some(character) = repository
        .latest_authorized_character()
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(AuthStatusView::not_authorized());
    };
    let latest_sync = repository
        .latest_character_order_sync(character.character_id)
        .await
        .map_err(|error| error.to_string())?;

    Ok(AuthStatusView {
        status: "authorized".to_string(),
        configured: true,
        character_id: Some(character.character_id),
        character_name: Some(character.character_name),
        latest_sync_status: latest_sync.as_ref().map(|sync| sync.status.clone()),
        latest_sync_completed_at: latest_sync.and_then(|sync| sync.completed_at),
        message: None,
    })
}

async fn get_auth_status_from_env() -> Result<AuthStatusView, String> {
    if let Some(error) = auth_env_config_error_from_sources(|name| std::env::var(name).ok()) {
        return Ok(AuthStatusView::not_configured(error));
    }

    let repository = auth_repository_from_env().await?;
    auth_status_from_repository(&repository).await
}

fn auth_env_config_error_from_sources<F>(mut source: F) -> Option<String>
where
    F: FnMut(&str) -> Option<String>,
{
    if let Err(error) = EveSsoConfig::from_sources(|name| source(name)) {
        return Some(error);
    }
    let database_url = source(DATABASE_URL_ENV)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    database_url
        .is_none()
        .then(|| format!("{DATABASE_URL_ENV} is required"))
}

fn esi_client_from_env() -> evetools_esi::EsiClient {
    std::env::var(ESI_BASE_URL_ENV)
        .ok()
        .and_then(|value| normalize_base_url(&value))
        .map(evetools_esi::EsiClient::new)
        .unwrap_or_else(evetools_esi::EsiClient::tranquility)
}

struct SsoCallbackListener {
    listener: TcpListener,
    callback_path: String,
}

async fn bind_sso_callback_listener(redirect_uri: &str) -> Result<SsoCallbackListener, String> {
    let url =
        Url::parse(redirect_uri).map_err(|error| format!("invalid SSO redirect URI: {error}"))?;
    let host = url
        .host_str()
        .ok_or_else(|| "SSO redirect URI must include a host".to_string())?;
    if host != "127.0.0.1" && host != "localhost" {
        return Err("desktop SSO redirect URI must use 127.0.0.1 or localhost".to_string());
    }
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "SSO redirect URI must include a port".to_string())?;
    let listener = TcpListener::bind(format!("{host}:{port}"))
        .await
        .map_err(|error| format!("failed to bind SSO callback listener: {error}"))?;
    Ok(SsoCallbackListener {
        listener,
        callback_path: url.path().to_string(),
    })
}

async fn wait_for_sso_callback(
    callback: SsoCallbackListener,
    expected_state: &str,
) -> Result<String, String> {
    let accepted = tokio::time::timeout(Duration::from_secs(180), callback.listener.accept())
        .await
        .map_err(|_| "timed out waiting for EVE SSO callback".to_string())?
        .map_err(|error| format!("failed to accept EVE SSO callback: {error}"))?;
    let (mut socket, _) = accepted;
    let mut buffer = [0_u8; 8192];
    let read = socket
        .read(&mut buffer)
        .await
        .map_err(|error| format!("failed to read EVE SSO callback: {error}"))?;
    let request = String::from_utf8_lossy(&buffer[..read]);
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| "invalid EVE SSO callback request".to_string())?;
    let parsed = Url::parse(&format!("http://127.0.0.1{target}"))
        .map_err(|error| format!("invalid EVE SSO callback URL: {error}"))?;
    let response = if parsed.path() == callback.callback_path {
        "EveTools authorization complete. You can return to the app."
    } else {
        "EveTools authorization failed. Unexpected callback path."
    };
    let _ = write_callback_response(&mut socket, response).await;
    if parsed.path() != callback.callback_path {
        return Err("EVE SSO callback path did not match configured redirect URI".to_string());
    }

    let mut code = None;
    let mut state = None;
    let mut callback_error = None;
    for (key, value) in parsed.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.to_string()),
            "state" => state = Some(value.to_string()),
            "error" => callback_error = Some(value.to_string()),
            _ => {}
        }
    }
    if let Some(error) = callback_error {
        return Err(format!("EVE SSO returned an error: {error}"));
    }
    if state.as_deref() != Some(expected_state) {
        return Err("EVE SSO state did not match".to_string());
    }
    code.ok_or_else(|| "EVE SSO callback did not include an authorization code".to_string())
}

async fn write_callback_response(
    socket: &mut tokio::net::TcpStream,
    body: &str,
) -> Result<(), std::io::Error> {
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/plain; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    socket.write_all(response.as_bytes()).await
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BrowserCommandSpec {
    program: String,
    args: Vec<String>,
}

fn system_browser_command_spec(url: &str) -> BrowserCommandSpec {
    #[cfg(target_os = "windows")]
    {
        return BrowserCommandSpec {
            program: "rundll32".to_string(),
            args: vec!["url.dll,FileProtocolHandler".to_string(), url.to_string()],
        };
    }

    #[cfg(target_os = "macos")]
    {
        return BrowserCommandSpec {
            program: "open".to_string(),
            args: vec![url.to_string()],
        };
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        BrowserCommandSpec {
            program: "xdg-open".to_string(),
            args: vec![url.to_string()],
        }
    }
}

fn open_system_browser(url: &str) -> Result<(), String> {
    let spec = system_browser_command_spec(url);

    Command::new(&spec.program)
        .args(&spec.args)
        .spawn()
        .map_err(|error| format!("failed to open system browser: {error}"))?;
    Ok(())
}

#[tauri::command]
async fn get_auth_status() -> Result<AuthStatusView, String> {
    get_auth_status_from_env().await
}

#[tauri::command]
async fn start_eve_sso_login() -> Result<AuthStatusView, String> {
    let config = EveSsoConfig::from_env()?;
    let repository = auth_repository_from_env().await?;
    let callback = bind_sso_callback_listener(&config.redirect_uri).await?;
    let verifier = generate_pkce_verifier();
    let challenge = pkce_challenge(&verifier);
    let state = generate_oauth_state();
    let auth_url = build_sso_authorization_url(&config, &state, &challenge)?;
    open_system_browser(&auth_url)?;

    let code = wait_for_sso_callback(callback, &state).await?;
    let token = esi_client_from_env()
        .exchange_authorization_code(
            &config.sso_base_url,
            &config.client_id,
            &code,
            &config.redirect_uri,
            &verifier,
        )
        .await
        .map_err(|error| error.to_string())?;
    let identity = decode_eve_access_token_identity(&token.access_token)?;
    if !identity
        .scopes
        .iter()
        .any(|scope| scope == REQUIRED_CHARACTER_ORDER_SCOPE)
    {
        return Err(format!(
            "EVE SSO token is missing required scope {REQUIRED_CHARACTER_ORDER_SCOPE}"
        ));
    }
    let refresh_token = token
        .refresh_token
        .ok_or_else(|| "EVE SSO response did not include a refresh token".to_string())?;
    repository
        .upsert_authorized_character(&AuthorizedCharacter {
            character_id: identity.character_id,
            character_name: identity.character_name,
            owner_hash: identity.owner_hash,
            last_login_at: chrono::Utc::now().to_rfc3339(),
        })
        .await
        .map_err(|error| error.to_string())?;
    repository
        .upsert_auth_token(&CharacterAuthToken {
            character_id: identity.character_id,
            refresh_token,
            access_token: Some(token.access_token),
            access_token_expires_at: Some(
                (chrono::Utc::now() + chrono::Duration::seconds(token.expires_in)).to_rfc3339(),
            ),
            scopes: identity.scopes,
            token_type: token.token_type,
        })
        .await
        .map_err(|error| error.to_string())?;

    auth_status_from_repository(&repository).await
}

#[tauri::command]
async fn sync_character_orders() -> Result<AuthenticatedOrderSyncSummary, String> {
    let config = EveSsoConfig::from_env()?;
    let repository = auth_repository_from_env().await?;
    let character = repository
        .latest_authorized_character()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "EVE SSO authorization is required".to_string())?;
    sync_authenticated_character_orders(
        &repository,
        &esi_client_from_env(),
        &config.sso_base_url,
        &config.client_id,
        character.character_id,
    )
    .await
    .map_err(|error| error.to_string())
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

    async fn order_monitor_items(
        &self,
        character_id: i64,
        language: &str,
    ) -> Result<Vec<OrderMonitorView>, String> {
        self.get_json(
            &format!("/characters/{character_id}/order-monitor"),
            &OrderMonitorHttpQuery {
                language,
                limit: 500,
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
        match self
            .http
            .get(format!("{}{}", self.base_url, path))
            .send()
            .await
        {
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

#[derive(Serialize)]
struct OrderMonitorHttpQuery<'a> {
    language: &'a str,
    limit: i64,
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
            get_auth_status,
            start_eve_sso_login,
            sync_character_orders,
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
        assert_eq!(
            list_order_monitor_items_with_source(MarketSource::Fixture)
                .await
                .unwrap()
                .len(),
            2
        );
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
        assert!(status
            .probes
            .iter()
            .all(|probe| probe.status == "not-configured"));
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

    #[test]
    fn sso_config_requires_client_id_and_redirect_uri() {
        let error = EveSsoConfig::from_sources(|_| None).unwrap_err();

        assert_eq!(error, "EVETOOLS_SSO_CLIENT_ID is required");
    }

    #[test]
    fn auth_env_config_reports_missing_sso_before_database() {
        let error = auth_env_config_error_from_sources(|_| None).unwrap();

        assert_eq!(error, "EVETOOLS_SSO_CLIENT_ID is required");
    }

    #[test]
    fn auth_env_config_requires_database_after_sso_config() {
        let error = auth_env_config_error_from_sources(|name| match name {
            "EVETOOLS_SSO_CLIENT_ID" => Some("client-id".to_string()),
            "EVETOOLS_SSO_REDIRECT_URI" => Some("http://127.0.0.1:17813/callback".to_string()),
            _ => None,
        })
        .unwrap();

        assert_eq!(error, "EVETOOLS_DATABASE_URL is required");
    }

    #[test]
    fn builds_sso_authorization_url_with_pkce_parameters() {
        let config = EveSsoConfig::from_sources(|name| match name {
            "EVETOOLS_SSO_CLIENT_ID" => Some("client-id".to_string()),
            "EVETOOLS_SSO_REDIRECT_URI" => Some("http://127.0.0.1:17813/callback".to_string()),
            "EVETOOLS_SSO_BASE_URL" => Some("https://login.example.test/".to_string()),
            _ => None,
        })
        .unwrap();

        let url = build_sso_authorization_url(&config, "state-123", "challenge-123").unwrap();

        assert!(url.starts_with("https://login.example.test/v2/oauth/authorize/"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=client-id"));
        assert!(url.contains("state=state-123"));
        assert!(url.contains("code_challenge=challenge-123"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("esi-markets.read_character_orders.v1"));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn windows_browser_command_avoids_cmd_shell_for_oauth_url() {
        let url = "https://login.example.test/v2/oauth/authorize/?client_id=a&state=b";
        let spec = system_browser_command_spec(url);

        assert_eq!(spec.program, "rundll32");
        assert_eq!(
            spec.args,
            vec!["url.dll,FileProtocolHandler".to_string(), url.to_string()]
        );
    }

    #[test]
    fn decodes_eve_access_token_identity() {
        let token = fake_access_token(serde_json::json!({
            "sub": "CHARACTER:EVE:90000001",
            "name": "Market Pilot",
            "owner": "owner-hash",
            "scp": ["esi-markets.read_character_orders.v1"]
        }));

        let identity = decode_eve_access_token_identity(&token).unwrap();

        assert_eq!(identity.character_id, 90_000_001);
        assert_eq!(identity.character_name, "Market Pilot");
        assert_eq!(identity.owner_hash.as_deref(), Some("owner-hash"));
        assert_eq!(
            identity.scopes,
            vec!["esi-markets.read_character_orders.v1".to_string()]
        );
    }

    fn fake_access_token(payload: serde_json::Value) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

        let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(payload.to_string());
        format!("{header}.{payload}.signature")
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
