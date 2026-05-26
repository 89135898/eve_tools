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
  type_id: number;
  item_name: string;
  recommended_entry_price: string;
  recommended_exit_price: string;
  net_profit: string;
  attention_score: number;
  liquidity_score: number;
  confidence_score: number;
  reason_codes: string[];
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

export function listSelectionCandidates(): Promise<SelectionCandidateView[]> {
  return invoke<SelectionCandidateView[]>("list_selection_candidates");
}

export function listOrderMonitorItems(): Promise<OrderMonitorView[]> {
  return invoke<OrderMonitorView[]>("list_order_monitor_items");
}

export function getSyncStatus(): Promise<SyncStatus> {
  return invoke<SyncStatus>("get_sync_status");
}
