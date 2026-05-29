use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const DEFAULT_PUBLIC_MARKET_REGION_ID: i32 = 10000002;
const DEFAULT_STARTED_BY: &str = "evetools-worker";
const DEFAULT_LEASE_TTL_SECONDS: i64 = 20 * 60;
const DATABASE_URL_ENV: &str = "EVETOOLS_DATABASE_URL";
const ESI_BASE_URL_ENV: &str = "EVETOOLS_ESI_BASE_URL";

#[derive(Debug, thiserror::Error)]
pub enum PublicMarketSyncError {
    #[error("market database error: {0}")]
    MarketDb(#[from] evetools_db::MarketDbError),
    #[error("public ESI error: {0}")]
    Esi(#[from] evetools_esi::EsiError),
}

#[derive(Debug, thiserror::Error)]
pub enum AuthenticatedOrderSyncError {
    #[error("auth database error: {0}")]
    AuthDb(#[from] evetools_db::AuthDbError),
    #[error("authenticated ESI error: {0}")]
    Esi(#[from] evetools_esi::EsiError),
    #[error("no auth token stored for character {character_id}")]
    MissingAuthToken { character_id: i64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticatedOrderSyncSummary {
    pub sync_run_id: i64,
    pub character_id: i64,
    pub status: String,
    pub order_count: i64,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum PublicMarketSyncCliError {
    #[error("EVETOOLS_DATABASE_URL is required")]
    MissingDatabaseUrl,
    #[error("invalid region id {value:?}")]
    InvalidRegionId { value: String },
    #[error("{flag} requires a value")]
    MissingArgument { flag: String },
    #[error("invalid value for {flag}: {value:?}")]
    InvalidFlagValue { flag: String, value: String },
    #[error("--all-default-regions cannot be combined with a region id")]
    ConflictingRegionSelection,
    #[error("unexpected argument {value:?}")]
    UnexpectedArgument { value: String },
    #[error("sql connection error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("sql migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("json serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("public market sync error: {0}")]
    Sync(#[from] PublicMarketSyncError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicMarketSyncCliConfig {
    pub database_url: String,
    pub esi_base_url: Option<String>,
    pub region_id: Option<i32>,
    pub all_default_regions: bool,
    pub started_by: String,
    pub lease_ttl_seconds: i64,
    pub max_age_seconds: Option<i64>,
    pub json: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicMarketSyncSummary {
    pub sync_run_id: Option<i64>,
    pub region_id: i32,
    pub status: String,
    pub order_count: i64,
    pub page_count: i32,
    pub message: String,
}

impl PublicMarketSyncCliConfig {
    pub fn from_env_and_args<I, S>(args: I) -> Result<Self, PublicMarketSyncCliError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::from_args_and_env(args, |name| std::env::var(name).ok())
    }

    pub fn from_args_and_env<I, S, F>(args: I, mut env: F) -> Result<Self, PublicMarketSyncCliError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
        F: FnMut(&str) -> Option<String>,
    {
        let database_url = env(DATABASE_URL_ENV)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or(PublicMarketSyncCliError::MissingDatabaseUrl)?;
        let esi_base_url = env(ESI_BASE_URL_ENV)
            .map(|value| value.trim().trim_end_matches('/').to_string())
            .filter(|value| !value.is_empty());
        let parsed_args = parse_public_market_sync_args(args)?;

        Ok(Self {
            database_url,
            esi_base_url,
            region_id: parsed_args.region_id,
            all_default_regions: parsed_args.all_default_regions,
            started_by: parsed_args.started_by,
            lease_ttl_seconds: parsed_args.lease_ttl_seconds,
            max_age_seconds: parsed_args.max_age_seconds,
            json: parsed_args.json,
        })
    }
}

pub async fn run_public_market_region_sync(
    config: PublicMarketSyncCliConfig,
) -> Result<Vec<PublicMarketSyncSummary>, PublicMarketSyncCliError> {
    let pool = evetools_db::connect_pool(&config.database_url).await?;
    evetools_db::migrate_catalog_schema(&pool).await?;
    let repository = evetools_db::MarketRepository::new(pool);
    let client = config
        .esi_base_url
        .as_deref()
        .map(evetools_esi::EsiClient::new)
        .unwrap_or_else(evetools_esi::EsiClient::tranquility);
    let hubs = default_trade_hubs();
    let region_ids = if config.all_default_regions {
        default_region_ids(&hubs)
    } else {
        vec![config.region_id.unwrap_or(DEFAULT_PUBLIC_MARKET_REGION_ID)]
    };

    let mut summaries = Vec::with_capacity(region_ids.len());
    for region_id in region_ids {
        summaries.push(
            sync_public_market_region_orders(
                &repository,
                &client,
                region_id,
                &hubs,
                &config.started_by,
                config.lease_ttl_seconds,
                config.max_age_seconds,
            )
            .await?,
        );
    }

    Ok(summaries)
}

pub fn public_market_sync_summaries_json(
    summaries: &[PublicMarketSyncSummary],
) -> Result<String, PublicMarketSyncCliError> {
    Ok(serde_json::to_string(summaries)?)
}

pub fn format_public_market_sync_summary(summary: &PublicMarketSyncSummary) -> String {
    let sync_run = summary
        .sync_run_id
        .map(|sync_run_id| sync_run_id.to_string())
        .unwrap_or_else(|| "none".to_string());
    format!(
        "{} public market region {} with sync_run_id {}: {} (orders={}, pages={})",
        summary.status,
        summary.region_id,
        sync_run,
        summary.message,
        summary.order_count,
        summary.page_count
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ParsedPublicMarketSyncArgs {
    region_id: Option<i32>,
    all_default_regions: bool,
    started_by: String,
    lease_ttl_seconds: i64,
    max_age_seconds: Option<i64>,
    json: bool,
}

fn parse_public_market_sync_args<I, S>(
    args: I,
) -> Result<ParsedPublicMarketSyncArgs, PublicMarketSyncCliError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut explicit_region_id = None;
    let mut all_default_regions = false;
    let mut started_by = DEFAULT_STARTED_BY.to_string();
    let mut lease_ttl_seconds = DEFAULT_LEASE_TTL_SECONDS;
    let mut max_age_seconds = None;
    let mut json = false;
    let mut args = args.into_iter().map(Into::into);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--all-default-regions" => {
                all_default_regions = true;
            }
            "--json" => {
                json = true;
            }
            "--region-id" => {
                let value = next_arg_value("--region-id", &mut args)?;
                set_region_id(&mut explicit_region_id, &value)?;
            }
            "--started-by" => {
                started_by = parse_non_empty_flag_value(
                    "--started-by",
                    &next_arg_value("--started-by", &mut args)?,
                )?;
            }
            "--lease-ttl-seconds" => {
                lease_ttl_seconds = parse_positive_i64_flag_value(
                    "--lease-ttl-seconds",
                    &next_arg_value("--lease-ttl-seconds", &mut args)?,
                )?;
            }
            "--max-age-seconds" => {
                max_age_seconds = Some(parse_non_negative_i64_flag_value(
                    "--max-age-seconds",
                    &next_arg_value("--max-age-seconds", &mut args)?,
                )?);
            }
            value if value.starts_with("--region-id=") => {
                let value = value.trim_start_matches("--region-id=");
                set_region_id(&mut explicit_region_id, value)?;
            }
            value if value.starts_with("--started-by=") => {
                let value = value.trim_start_matches("--started-by=");
                started_by = parse_non_empty_flag_value("--started-by", value)?;
            }
            value if value.starts_with("--lease-ttl-seconds=") => {
                let value = value.trim_start_matches("--lease-ttl-seconds=");
                lease_ttl_seconds = parse_positive_i64_flag_value("--lease-ttl-seconds", value)?;
            }
            value if value.starts_with("--max-age-seconds=") => {
                let value = value.trim_start_matches("--max-age-seconds=");
                max_age_seconds = Some(parse_non_negative_i64_flag_value(
                    "--max-age-seconds",
                    value,
                )?);
            }
            value if value.starts_with('-') => {
                return Err(PublicMarketSyncCliError::UnexpectedArgument {
                    value: value.to_string(),
                });
            }
            value => {
                set_region_id(&mut explicit_region_id, value)?;
            }
        }
    }

    if all_default_regions && explicit_region_id.is_some() {
        return Err(PublicMarketSyncCliError::ConflictingRegionSelection);
    }

    Ok(ParsedPublicMarketSyncArgs {
        region_id: if all_default_regions {
            None
        } else {
            Some(explicit_region_id.unwrap_or(DEFAULT_PUBLIC_MARKET_REGION_ID))
        },
        all_default_regions,
        started_by,
        lease_ttl_seconds,
        max_age_seconds,
        json,
    })
}

fn next_arg_value(
    flag: &str,
    args: &mut impl Iterator<Item = String>,
) -> Result<String, PublicMarketSyncCliError> {
    args.next()
        .ok_or_else(|| PublicMarketSyncCliError::MissingArgument {
            flag: flag.to_string(),
        })
}

fn set_region_id(region_id: &mut Option<i32>, value: &str) -> Result<(), PublicMarketSyncCliError> {
    if region_id.is_some() {
        return Err(PublicMarketSyncCliError::UnexpectedArgument {
            value: value.to_string(),
        });
    }
    *region_id = Some(parse_region_id(value)?);
    Ok(())
}

fn parse_non_empty_flag_value(flag: &str, value: &str) -> Result<String, PublicMarketSyncCliError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(PublicMarketSyncCliError::InvalidFlagValue {
            flag: flag.to_string(),
            value: value.to_string(),
        });
    }
    Ok(value.to_string())
}

fn parse_positive_i64_flag_value(flag: &str, value: &str) -> Result<i64, PublicMarketSyncCliError> {
    let parsed =
        value
            .trim()
            .parse::<i64>()
            .map_err(|_| PublicMarketSyncCliError::InvalidFlagValue {
                flag: flag.to_string(),
                value: value.to_string(),
            })?;
    if parsed <= 0 {
        return Err(PublicMarketSyncCliError::InvalidFlagValue {
            flag: flag.to_string(),
            value: value.to_string(),
        });
    }
    Ok(parsed)
}

fn parse_non_negative_i64_flag_value(
    flag: &str,
    value: &str,
) -> Result<i64, PublicMarketSyncCliError> {
    let parsed =
        value
            .trim()
            .parse::<i64>()
            .map_err(|_| PublicMarketSyncCliError::InvalidFlagValue {
                flag: flag.to_string(),
                value: value.to_string(),
            })?;
    if parsed < 0 {
        return Err(PublicMarketSyncCliError::InvalidFlagValue {
            flag: flag.to_string(),
            value: value.to_string(),
        });
    }
    Ok(parsed)
}

fn parse_region_id(value: &str) -> Result<i32, PublicMarketSyncCliError> {
    value
        .trim()
        .parse::<i32>()
        .map_err(|_| PublicMarketSyncCliError::InvalidRegionId {
            value: value.to_string(),
        })
}

fn default_region_ids(hubs: &[TradeHubConfig]) -> Vec<i32> {
    let mut hubs_with_order: Vec<_> = hubs.iter().enumerate().collect();
    hubs_with_order.sort_by_key(|(index, hub)| (hub.sort_order, *index));

    let mut seen = HashSet::new();
    hubs_with_order
        .into_iter()
        .filter_map(|(_, hub)| {
            if seen.insert(hub.region_id) {
                Some(hub.region_id)
            } else {
                None
            }
        })
        .collect()
}

pub async fn sync_public_market_region_orders(
    repository: &evetools_db::MarketRepository,
    client: &evetools_esi::EsiClient,
    region_id: i32,
    hubs: &[TradeHubConfig],
    started_by: &str,
    lease_ttl_seconds: i64,
    max_age_seconds: Option<i64>,
) -> Result<PublicMarketSyncSummary, PublicMarketSyncError> {
    repository
        .upsert_trade_hubs(&trade_hub_configs_as_db_records(hubs))
        .await?;

    if let Some(summary) = should_skip_region(repository, region_id, max_age_seconds).await? {
        return Ok(summary);
    }

    let lease_owner = format!("evetools-worker:{}:{}", std::process::id(), region_id);
    let lease = repository
        .try_start_sync_run(
            region_id,
            "public-esi",
            started_by,
            &lease_owner,
            chrono::Duration::seconds(lease_ttl_seconds),
        )
        .await?;
    if lease.status == evetools_db::MarketSyncStartStatus::AlreadyRunning {
        return Ok(PublicMarketSyncSummary {
            sync_run_id: lease.sync_run_id,
            region_id,
            status: "already-running".to_string(),
            order_count: 0,
            page_count: 0,
            message: lease.message,
        });
    }

    let sync_run_id = lease.sync_run_id.expect("started sync runs include an id");
    repository.mark_sync_run_running(sync_run_id).await?;

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
    let order_count = snapshots.len() as i64;
    repository
        .complete_sync_run(sync_run_id, 0, order_count)
        .await?;

    Ok(PublicMarketSyncSummary {
        sync_run_id: Some(sync_run_id),
        region_id,
        status: "success".to_string(),
        order_count,
        page_count: 0,
        message: "synced".to_string(),
    })
}

pub async fn sync_authenticated_character_orders(
    repository: &evetools_db::AuthRepository,
    client: &evetools_esi::EsiClient,
    sso_base_url: &str,
    client_id: &str,
    character_id: i64,
) -> Result<AuthenticatedOrderSyncSummary, AuthenticatedOrderSyncError> {
    let mut token = repository
        .auth_token(character_id)
        .await?
        .ok_or(AuthenticatedOrderSyncError::MissingAuthToken { character_id })?;
    let access_token = if token_access_token_is_fresh(&token) {
        token.access_token.clone().unwrap_or_default()
    } else {
        let refreshed = client
            .refresh_access_token(sso_base_url, client_id, &token.refresh_token)
            .await?;
        token.access_token = Some(refreshed.access_token.clone());
        token.refresh_token = refreshed.refresh_token.unwrap_or(token.refresh_token);
        token.access_token_expires_at =
            Some((Utc::now() + chrono::Duration::seconds(refreshed.expires_in)).to_rfc3339());
        token.token_type = refreshed.token_type;
        repository.upsert_auth_token(&token).await?;
        token.access_token.clone().unwrap_or_default()
    };

    let sync_run_id = repository.start_character_order_sync(character_id).await?;
    let orders = match client.character_orders(character_id, &access_token).await {
        Ok(orders) => orders,
        Err(error) => {
            let _ = repository
                .fail_character_order_sync(sync_run_id, &error.to_string())
                .await;
            return Err(error.into());
        }
    };

    let snapshots = character_order_snapshots(sync_run_id, character_id, &orders);
    if let Err(error) = repository
        .replace_character_order_snapshots(sync_run_id, &snapshots)
        .await
    {
        let _ = repository
            .fail_character_order_sync(sync_run_id, &error.to_string())
            .await;
        return Err(error.into());
    }

    let order_count = snapshots.len() as i64;
    repository
        .complete_character_order_sync(sync_run_id, order_count)
        .await?;

    Ok(AuthenticatedOrderSyncSummary {
        sync_run_id,
        character_id,
        status: "success".to_string(),
        order_count,
        message: "synced".to_string(),
    })
}

fn token_access_token_is_fresh(token: &evetools_db::CharacterAuthToken) -> bool {
    let Some(access_token) = token.access_token.as_deref() else {
        return false;
    };
    if access_token.trim().is_empty() {
        return false;
    }
    let Some(expires_at) = token.access_token_expires_at.as_deref() else {
        return false;
    };
    let Ok(expires_at) = DateTime::parse_from_rfc3339(expires_at) else {
        return false;
    };
    expires_at.with_timezone(&Utc) > Utc::now() + chrono::Duration::seconds(60)
}

fn character_order_snapshots(
    sync_run_id: i64,
    character_id: i64,
    orders: &[evetools_esi::EsiCharacterOrder],
) -> Vec<evetools_db::CharacterOrderSnapshotInput> {
    orders
        .iter()
        .map(|order| evetools_db::CharacterOrderSnapshotInput {
            sync_run_id,
            character_id,
            order_id: order.order_id,
            type_id: order.type_id,
            region_id: order.region_id,
            location_id: order.location_id,
            is_buy_order: order.is_buy_order,
            price: order.price,
            volume_remain: i64::from(order.volume_remain),
            volume_total: i64::from(order.volume_total),
            issued: order.issued.clone(),
            duration: order.duration,
            min_volume: order.min_volume,
            order_range: order.range.clone(),
            is_corporation: order.is_corporation,
            escrow: order.escrow,
        })
        .collect()
}

async fn should_skip_region(
    repository: &evetools_db::MarketRepository,
    region_id: i32,
    max_age_seconds: Option<i64>,
) -> Result<Option<PublicMarketSyncSummary>, PublicMarketSyncError> {
    let Some(max_age_seconds) = max_age_seconds else {
        return Ok(None);
    };
    let health = repository.sync_health_at(chrono::Utc::now()).await?;
    let fresh_hub = health.hubs.into_iter().find(|hub| {
        hub.region_id == region_id
            && hub
                .age_seconds
                .is_some_and(|age_seconds| age_seconds <= max_age_seconds)
    });

    Ok(fresh_hub.map(|hub| PublicMarketSyncSummary {
        sync_run_id: hub.latest_success_sync_run_id,
        region_id,
        status: "skipped".to_string(),
        order_count: hub.order_count.unwrap_or(0),
        page_count: 0,
        message: format!(
            "latest successful sync is {} seconds old, within max-age {} seconds",
            hub.age_seconds.unwrap_or(0),
            max_age_seconds
        ),
    }))
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
    let mut seen_order_ids = HashSet::new();

    orders
        .iter()
        .filter(|order| {
            hub_station_ids.contains(&order.location_id) && seen_order_ids.insert(order.order_id)
        })
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
    fn cli_config_defaults_to_the_forge_and_requires_database_url() {
        let missing = PublicMarketSyncCliConfig::from_args_and_env(Vec::<String>::new(), |_| None)
            .unwrap_err();
        assert_eq!(missing.to_string(), "EVETOOLS_DATABASE_URL is required");

        let config = PublicMarketSyncCliConfig::from_args_and_env(Vec::<String>::new(), |name| {
            (name == DATABASE_URL_ENV).then(|| "postgresql://localhost/test".to_string())
        })
        .unwrap();

        assert_eq!(config.region_id, Some(10000002));
        assert!(!config.all_default_regions);
        assert_eq!(config.started_by, "evetools-worker");
        assert_eq!(config.lease_ttl_seconds, 1200);
        assert_eq!(config.max_age_seconds, None);
        assert!(!config.json);
        assert_eq!(config.database_url, "postgresql://localhost/test");
        assert_eq!(config.esi_base_url, None);
    }

    #[test]
    fn cli_config_accepts_region_arg_and_custom_esi_base_url() {
        let config =
            PublicMarketSyncCliConfig::from_args_and_env(["--region-id", "10000043"], |name| {
                match name {
                    DATABASE_URL_ENV => Some("postgresql://localhost/test".to_string()),
                    ESI_BASE_URL_ENV => Some("http://127.0.0.1:1234/".to_string()),
                    _ => None,
                }
            })
            .unwrap();

        assert_eq!(config.region_id, Some(10000043));
        assert_eq!(
            config.esi_base_url,
            Some("http://127.0.0.1:1234".to_string())
        );
    }

    #[test]
    fn cli_config_accepts_production_flags() {
        let config = PublicMarketSyncCliConfig::from_args_and_env(
            [
                "--all-default-regions",
                "--started-by",
                "cron/market",
                "--lease-ttl-seconds=600",
                "--max-age-seconds",
                "300",
                "--json",
            ],
            |name| match name {
                DATABASE_URL_ENV => Some("postgresql://user:secret@localhost/test".to_string()),
                ESI_BASE_URL_ENV => Some("http://127.0.0.1:1234/".to_string()),
                _ => None,
            },
        )
        .unwrap();

        assert_eq!(
            config.database_url,
            "postgresql://user:secret@localhost/test"
        );
        assert_eq!(
            config.esi_base_url,
            Some("http://127.0.0.1:1234".to_string())
        );
        assert_eq!(config.region_id, None);
        assert!(config.all_default_regions);
        assert_eq!(config.started_by, "cron/market");
        assert_eq!(config.lease_ttl_seconds, 600);
        assert_eq!(config.max_age_seconds, Some(300));
        assert!(config.json);
    }

    #[test]
    fn cli_config_rejects_invalid_region_args() {
        let error = PublicMarketSyncCliConfig::from_args_and_env(["abc"], |name| {
            (name == DATABASE_URL_ENV).then(|| "postgresql://localhost/test".to_string())
        })
        .unwrap_err();

        assert_eq!(error.to_string(), "invalid region id \"abc\"");
    }

    #[test]
    fn cli_config_rejects_all_default_regions_with_region_id() {
        let error = PublicMarketSyncCliConfig::from_args_and_env(
            ["--all-default-regions", "--region-id", "10000002"],
            |name| (name == DATABASE_URL_ENV).then(|| "postgresql://localhost/test".to_string()),
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "--all-default-regions cannot be combined with a region id"
        );
    }

    #[test]
    fn formats_public_market_sync_summary_for_cli_output() {
        assert_eq!(
            format_public_market_sync_summary(&PublicMarketSyncSummary {
                sync_run_id: Some(42),
                region_id: 10000002,
                status: "success".to_string(),
                order_count: 7,
                page_count: 1,
                message: "synced".to_string(),
            }),
            "success public market region 10000002 with sync_run_id 42: synced (orders=7, pages=1)"
        );
    }

    #[test]
    fn public_market_sync_summary_serializes_json_without_secrets() {
        let summary = PublicMarketSyncSummary {
            sync_run_id: Some(42),
            region_id: 10000002,
            status: "already-running".to_string(),
            order_count: 0,
            page_count: 0,
            message: "another sync is already running".to_string(),
        };

        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"region_id\":10000002"));
        assert!(json.contains("\"status\":\"already-running\""));
        assert!(!json.contains("postgres"));
        assert!(!json.contains("secret"));
        assert_eq!(
            serde_json::from_str::<PublicMarketSyncSummary>(&json).unwrap(),
            summary
        );
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
    fn default_region_ids_are_unique_in_trade_hub_order() {
        assert_eq!(
            default_region_ids(&default_trade_hubs()),
            vec![10000002, 10000043, 10000032, 10000030, 10000042]
        );
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

    #[test]
    fn market_order_snapshots_drop_duplicate_order_ids() {
        let mut duplicate = evetools_esi::EsiMarketOrder {
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
        };
        let first = duplicate.clone();
        duplicate.price = 5.02;

        let snapshots = market_order_snapshots_for_hubs(
            42,
            10000002,
            &[first, duplicate],
            &default_trade_hubs(),
        );

        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].order_id, 7_000_000_001);
        assert_eq!(snapshots[0].price, 5.01);
    }
}
