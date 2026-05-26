use chrono::Utc;
use evetools_domain::fixtures::{
    fixture_market_lookup, fixture_order_monitor, fixture_selection_candidates,
};
use evetools_domain::{
    build_selection_candidate, classify_price_trend, summarize_jita_market, FeeProfile,
    MarketLookupView, OrderMonitorView, PublicMarketHistoryDay, PublicMarketOrder,
    SelectionCandidateView, THE_FORGE_REGION_ID,
};
use evetools_esi::{EsiClient, EsiError, EsiMarketHistoryDay, EsiMarketOrder, EsiOrderType};
use evetools_worker::{
    fixture_fallback_sync_status, fixture_sync_status, live_sync_status, SyncStatus,
};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use std::sync::atomic::{AtomicBool, Ordering};

const SELECTION_SEED_TYPES: &[(i32, &str)] = &[
    (34, "Tritanium"),
    (35, "Pyerite"),
    (36, "Mexallon"),
    (37, "Isogen"),
];

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
async fn list_selection_candidates() -> Result<Vec<SelectionCandidateView>, String> {
    list_selection_candidates_with_source(MarketSource::from_env()).await
}

async fn list_selection_candidates_with_source(
    source: MarketSource,
) -> Result<Vec<SelectionCandidateView>, String> {
    match source {
        MarketSource::Fixture => {
            mark_public_market_fallback(false);
            Ok(fixture_selection_candidates())
        }
        MarketSource::Live(client) => {
            let mut candidates = Vec::new();
            for (type_id, item_name) in SELECTION_SEED_TYPES {
                if let Ok(candidate) = selection_candidate_live(*type_id, item_name, &client).await
                {
                    candidates.push(candidate);
                }
            }

            if candidates.is_empty() {
                mark_public_market_fallback(true);
                Ok(fixture_selection_candidates())
            } else {
                mark_public_market_fallback(false);
                candidates.sort_by(|left, right| {
                    right
                        .attention_score
                        .cmp(&left.attention_score)
                        .then_with(|| left.item_name.cmp(&right.item_name))
                });
                Ok(candidates)
            }
        }
    }
}

async fn selection_candidate_live(
    type_id: i32,
    item_name: &str,
    client: &EsiClient,
) -> Result<SelectionCandidateView, EsiError> {
    let orders = client
        .market_orders(THE_FORGE_REGION_ID, type_id, EsiOrderType::All)
        .await?;
    let history = client.market_history(THE_FORGE_REGION_ID, type_id).await?;
    let domain_orders = to_domain_orders(&orders);
    let domain_history = to_domain_history(&history);
    let summary = summarize_jita_market(
        type_id,
        item_name,
        &domain_orders,
        &domain_history,
        Utc::now().to_rfc3339(),
    );
    let analysis = build_selection_candidate(&summary, &FeeProfile::conservative_default());

    Ok(SelectionCandidateView::from_analysis(analysis))
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

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            lookup_market_price,
            list_selection_candidates,
            list_order_monitor_items,
            get_sync_status
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
}
