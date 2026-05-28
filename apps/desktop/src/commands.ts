import { invoke } from "@tauri-apps/api/core";

export type MarketLookupView = {
  type_id: number;
  item_name: string;
  best_bid: string;
  best_ask: string;
  spread: string;
  spread_percent: string;
  daily_volume: number;
  price_trend: string;
  top_buy_depth: number;
  top_sell_depth: number;
  last_synced_at: string;
  data_quality: string;
};

export type SelectionCandidateView = {
  hub_id: string;
  hub_name: string;
  region_id: number;
  station_id: number;
  type_id: number;
  item_name: string;
  recommended_entry_price: string;
  recommended_exit_price: string;
  net_profit: string;
  attention_score: number;
  liquidity_score: number;
  confidence_score: number;
  reason_codes: string[];
  last_synced_at: string;
};

export type TradeHubView = {
  hub_id: string;
  display_name: string;
  region_id: number;
  system_id: number;
  station_id: number;
  enabled: boolean;
  sort_order: number;
};

export type OrderMonitorView = {
  order_id: string;
  type_id: number;
  item_name: string;
  side: string;
  current_price: string;
  market_leader_price: string;
  recommended_price: string;
  recommended_action: string;
  urgency_score: number;
  reason_codes: string[];
  stale_data_flag: boolean;
};

export type SyncStatus = {
  public_market_sync: string;
  authenticated_order_sync: string;
  data_source: string;
};

export function lookupMarketPrice(query: string): Promise<MarketLookupView> {
  return invoke<MarketLookupView>("lookup_market_price", { query });
}

export function listSelectionCandidates(
  language: string,
  hubIds: string[]
): Promise<SelectionCandidateView[]> {
  return invoke<SelectionCandidateView[]>("list_selection_candidates", { language, hubIds });
}

export function listTradeHubs(): Promise<TradeHubView[]> {
  return invoke<TradeHubView[]>("list_trade_hubs");
}

export function listOrderMonitorItems(): Promise<OrderMonitorView[]> {
  return invoke<OrderMonitorView[]>("list_order_monitor_items");
}

export function getSyncStatus(): Promise<SyncStatus> {
  return invoke<SyncStatus>("get_sync_status");
}

export type CatalogStatus = {
  status: string;
  build_number: number | null;
  release_date: string | null;
  source_url: string | null;
  completed_at: string | null;
  error_summary: string | null;
  type_count: number;
  group_count: number;
  category_count: number;
  market_group_count: number;
};

export type InventoryTypeView = {
  type_id: number;
  group_id: number;
  category_id: number | null;
  market_group_id: number | null;
  display_name: string;
  name_en: string | null;
  name_zh: string | null;
  group_name: string | null;
  category_name: string | null;
  market_group_name: string | null;
  published: boolean;
  market_eligible: boolean;
};

export function getSdeCatalogStatus(): Promise<CatalogStatus> {
  return invoke<CatalogStatus>("get_sde_catalog_status");
}

export function importSdeCatalogLatest(): Promise<CatalogStatus> {
  return invoke<CatalogStatus>("import_sde_catalog_latest");
}

export function searchInventoryTypes(
  query: string,
  language: string,
  limit = 20
): Promise<InventoryTypeView[]> {
  return invoke<InventoryTypeView[]>("search_inventory_types", { query, language, limit });
}

export function getInventoryType(
  typeId: number,
  language: string
): Promise<InventoryTypeView | null> {
  return invoke<InventoryTypeView | null>("get_inventory_type", { typeId, language });
}
