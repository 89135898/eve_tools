use evetools_domain::fixtures::{
    fixture_market_lookup, fixture_order_monitor, fixture_selection_candidates,
};
use evetools_domain::{MarketLookupView, OrderMonitorView, SelectionCandidateView};
use evetools_worker::{fixture_sync_status, SyncStatus};

#[tauri::command]
fn lookup_market_price(query: String) -> Result<MarketLookupView, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err("Item query is required".to_string());
    }
    Ok(fixture_market_lookup(trimmed))
}

#[tauri::command]
fn list_selection_candidates() -> Result<Vec<SelectionCandidateView>, String> {
    Ok(fixture_selection_candidates())
}

#[tauri::command]
fn list_order_monitor_items() -> Result<Vec<OrderMonitorView>, String> {
    Ok(fixture_order_monitor())
}

#[tauri::command]
fn get_sync_status() -> Result<SyncStatus, String> {
    Ok(fixture_sync_status())
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

    #[test]
    fn lookup_rejects_empty_query() {
        let result = lookup_market_price("   ".to_string());
        assert_eq!(result.unwrap_err(), "Item query is required");
    }

    #[test]
    fn fixture_commands_return_mvp_views() {
        assert_eq!(
            lookup_market_price("Tritanium".to_string())
                .unwrap()
                .item_name,
            "Tritanium"
        );
        assert_eq!(list_selection_candidates().unwrap().len(), 2);
        assert_eq!(list_order_monitor_items().unwrap().len(), 2);
        assert_eq!(get_sync_status().unwrap().public_market_sync, "fixture-ready");
    }
}
