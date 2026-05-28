use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use evetools_api::{
    ApiError, EveToolsReadApi, InventoryTypeLookupRequest, InventoryTypeSearchRequest,
    SelectionCandidatesRequest, StationOrdersRequest,
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, str::FromStr};
use thiserror::Error;

const DATABASE_URL_ENV: &str = "EVETOOLS_DATABASE_URL";
const HTTP_ADDR_ENV: &str = "EVETOOLS_HTTP_ADDR";
const DEFAULT_HTTP_ADDR: &str = "127.0.0.1:8080";
const DEFAULT_LANGUAGE: &str = "en-US";
const DEFAULT_SEARCH_LIMIT: i64 = 20;
const DEFAULT_STATION_ORDERS_LIMIT: i64 = 100;
const DEFAULT_SELECTION_LIMIT_PER_HUB: i64 = 25;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpApiConfig {
    pub database_url: String,
    pub bind_addr: SocketAddr,
}

#[derive(Debug, Error)]
pub enum HttpApiConfigError {
    #[error("EVETOOLS_DATABASE_URL is required")]
    MissingDatabaseUrl,
    #[error("invalid EVETOOLS_HTTP_ADDR {value:?}: {source}")]
    InvalidBindAddr {
        value: String,
        source: std::net::AddrParseError,
    },
}

#[derive(Debug, Error)]
pub enum HttpApiServeError {
    #[error("api config error: {0}")]
    Config(#[from] HttpApiConfigError),
    #[error("api initialization error: {0}")]
    Api(#[from] ApiError),
    #[error("failed to bind HTTP listener: {0}")]
    Bind(std::io::Error),
    #[error("HTTP server error: {0}")]
    Server(std::io::Error),
}

impl HttpApiConfig {
    pub fn from_env() -> Result<Self, HttpApiConfigError> {
        Self::from_env_reader(|name| std::env::var(name).ok())
    }

    pub fn from_env_reader<F>(mut env: F) -> Result<Self, HttpApiConfigError>
    where
        F: FnMut(&str) -> Option<String>,
    {
        let database_url = env(DATABASE_URL_ENV)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or(HttpApiConfigError::MissingDatabaseUrl)?;
        let bind_addr_value = env(HTTP_ADDR_ENV)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_HTTP_ADDR.to_string());
        let bind_addr = SocketAddr::from_str(&bind_addr_value).map_err(|source| {
            HttpApiConfigError::InvalidBindAddr {
                value: bind_addr_value,
                source,
            }
        })?;

        Ok(Self {
            database_url,
            bind_addr,
        })
    }
}

pub async fn serve_from_env() -> Result<(), HttpApiServeError> {
    serve(HttpApiConfig::from_env()?).await
}

pub async fn serve(config: HttpApiConfig) -> Result<(), HttpApiServeError> {
    let api = EveToolsReadApi::connect(&config.database_url).await?;
    let listener = tokio::net::TcpListener::bind(config.bind_addr)
        .await
        .map_err(HttpApiServeError::Bind)?;
    axum::serve(listener, build_router(api))
        .await
        .map_err(HttpApiServeError::Server)
}

pub fn build_router(api: EveToolsReadApi) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/catalog/status", get(catalog_status))
        .route("/inventory-types/{type_id}", get(get_inventory_type))
        .route("/inventory-types/search", get(search_inventory_types))
        .route("/trade-hubs", get(list_trade_hubs))
        .route("/station-orders", get(latest_station_orders))
        .route("/selection-candidates", get(selection_candidates))
        .with_state(api)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
    })
}

async fn catalog_status(State(api): State<EveToolsReadApi>) -> Result<Response, HttpApiError> {
    Ok(Json(api.catalog_status().await?).into_response())
}

async fn get_inventory_type(
    State(api): State<EveToolsReadApi>,
    Path(type_id): Path<i32>,
    Query(query): Query<LanguageQuery>,
) -> Result<Response, HttpApiError> {
    Ok(Json(
        api.get_inventory_type(InventoryTypeLookupRequest {
            type_id,
            language: query.language(),
        })
        .await?,
    )
    .into_response())
}

async fn search_inventory_types(
    State(api): State<EveToolsReadApi>,
    Query(query): Query<InventoryTypeSearchQuery>,
) -> Result<Response, HttpApiError> {
    Ok(Json(
        api.search_inventory_types(InventoryTypeSearchRequest {
            query: query.query,
            language: query.language.unwrap_or_else(default_language),
            limit: query.limit.unwrap_or(DEFAULT_SEARCH_LIMIT),
        })
        .await?,
    )
    .into_response())
}

async fn list_trade_hubs(State(api): State<EveToolsReadApi>) -> Result<Response, HttpApiError> {
    Ok(Json(api.list_trade_hubs().await?).into_response())
}

async fn latest_station_orders(
    State(api): State<EveToolsReadApi>,
    Query(query): Query<StationOrdersQuery>,
) -> Result<Response, HttpApiError> {
    Ok(Json(
        api.latest_station_orders(StationOrdersRequest {
            region_id: query.region_id,
            station_id: query.station_id,
            limit: query.limit.unwrap_or(DEFAULT_STATION_ORDERS_LIMIT),
        })
        .await?,
    )
    .into_response())
}

async fn selection_candidates(
    State(api): State<EveToolsReadApi>,
    Query(query): Query<SelectionCandidatesQuery>,
) -> Result<Response, HttpApiError> {
    Ok(Json(
        api.selection_candidates(SelectionCandidatesRequest {
            hub_ids: parse_hub_ids(query.hub_ids.as_deref()),
            language: query.language.unwrap_or_else(default_language),
            limit_per_hub: query
                .limit_per_hub
                .unwrap_or(DEFAULT_SELECTION_LIMIT_PER_HUB),
        })
        .await?,
    )
    .into_response())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct HealthResponse {
    status: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct LanguageQuery {
    language: Option<String>,
}

impl LanguageQuery {
    fn language(self) -> String {
        self.language.unwrap_or_else(default_language)
    }
}

#[derive(Clone, Debug, Deserialize)]
struct InventoryTypeSearchQuery {
    query: String,
    language: Option<String>,
    limit: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
struct StationOrdersQuery {
    region_id: i32,
    station_id: i64,
    limit: Option<i64>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct SelectionCandidatesQuery {
    hub_ids: Option<String>,
    language: Option<String>,
    limit_per_hub: Option<i64>,
}

#[derive(Debug)]
struct HttpApiError(ApiError);

impl From<ApiError> for HttpApiError {
    fn from(error: ApiError) -> Self {
        Self(error)
    }
}

impl IntoResponse for HttpApiError {
    fn into_response(self) -> Response {
        let status = StatusCode::INTERNAL_SERVER_ERROR;
        let body = Json(ErrorResponse {
            error: self.0.to_string(),
        });
        (status, body).into_response()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct ErrorResponse {
    error: String,
}

fn default_language() -> String {
    DEFAULT_LANGUAGE.to_string()
}

fn parse_hub_ids(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .filter_map(|hub_id| {
            let hub_id = hub_id.trim();
            (!hub_id.is_empty()).then(|| hub_id.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_bind_addr_and_requires_database_url() {
        let missing = HttpApiConfig::from_env_reader(|_| None).unwrap_err();
        assert_eq!(missing.to_string(), "EVETOOLS_DATABASE_URL is required");

        let config = HttpApiConfig::from_env_reader(|name| {
            (name == DATABASE_URL_ENV).then(|| "postgresql://localhost/test".to_string())
        })
        .unwrap();

        assert_eq!(config.database_url, "postgresql://localhost/test");
        assert_eq!(config.bind_addr, "127.0.0.1:8080".parse().unwrap());
    }

    #[test]
    fn parses_comma_separated_hub_ids() {
        assert_eq!(
            parse_hub_ids(Some("jita, amarr,,hek")),
            vec!["jita", "amarr", "hek"]
        );
    }
}
